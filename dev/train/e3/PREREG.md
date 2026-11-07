# E3: a Panel against the daemon Council, after the E2 fixes (pre-registration)

`specs/089` section 8 (slice P6). Written and committed **before any run**, and before the scorer, its tests and the runbook exist
in this directory. The questions, arms, counts, measures, thresholds, caps and stop rules below are fixed; changing any of them
after the first run starts voids the experiment (start a new one and say so). Registered 2026-11-07. **Nothing in this file has
been run.** The run needs the operator's separate, explicit, written "run E3" (RUNBOOK section 0); the operator's approval of this
design and of the budget on 2026-11-07 is not that go.

## Question

After the E2 fixes (spec 089 P1 to P4b), on the same kind of framed question, does a Panel run by Claude Code's own subagents
reach the same **decision class** as the daemon Council, produce **readable Round 2 critiques** as often, **escalate** about as
often, and raise at least one **P0 or P1** where the Council does, by more than the daemon Council differs from a second run of
itself? And does adding a Delphi-run **foreign red-team seat** to the Panel change that?

This tunes the section 4.5 notice only. The Panel stays the default and stays allowed for every question (086 OD-1); no outcome
below can change that. **Nothing about finding-level quality is claimed**: E3 does not measure whether the Panel found the same
findings as the Council (see "What changed from E2").

## What changed from E2 (the operator's decisions of 2026-11-07, not reopened here)

1. **Finding overlap and recall are dropped from the rule.** Matcher v2 failed its validation (agreement 0.1517 against the
   required 0.30; `dev/train/e2/RESCORE.md`), and v1 matched 0 of 40 reference findings even between two Council runs. E3 uses
   mechanical, class-level measures only.
2. **Two Panel arms**, Claude-only (A1) and with a foreign red-team seat (A2).
3. **Claude Code only.** No other host unless the S0b probe shows *parallel isolated subagents* `verified`; it has not been run
   live, so none is planned.
4. **Questions** are four that qualified under E2's rule plus two new ones chosen by a written rule (below).
5. **A smaller design** that fits the operator's caps (budget section).

## Arms

- **A1, Panel, Claude-only seats.** Claude Code's own subagents through the `delphi-council` skill (`council_frame`,
  `council_open`, `council_brief`, `council_submit`, `council_result`), `blind: yes`. As E2's arm A on the fixed build.
- **A2, Panel with a Delphi-run foreign red-team.** The same, with `council_open foreign_seat: red-team,
  operator_confirmed: true`. Delphi runs the red-team seat on another model family (the configured red-team family, Gemini via
  `agy` in the operator's configuration); its Delphi-side tokens count in the Delphi budget. The `operator_confirmed: true` is
  the operator's written word in the run prompt (a foreign seat spends Delphi tokens, spec 089 OD-1); it is host-reported.
- **B1, daemon Council** (`request_council`, Mode A), seats and families as configured. The reference for the decision class.
- **B2, repeat daemon Council**, a second fresh run of B1. **The noise floor:** how far B2 is from B1 is the most agreement a Panel
  can fairly be asked for. B2 is run on four of the six questions only (budget).
- **Same input.** Each question is one text file passed byte for byte to every run, in a new session, against the same repository
  commit (recorded in the manifest). Nothing carries over between runs; no Panel stance is shown to a Council and no Council
  record to a Panel; a Panel's escalation is never followed by a daemon Council inside the experiment. Each arm family uses its
  own scratch home (RUNBOOK section 2).
- **Venue is the harness's label, not the record's.** The arm comes from where the harness put the record (the manifest), and
  `venue:` and `blind:` are read from front matter only as a cross-check (E2 `score.py` already expects `venue: panel` /
  `venue: daemon` after spec 089 P2).

### Host and models

Claude Code only, `--model sonnet` as in E2; the seats' models are whatever the plugin agents declare (E2 saw opus, sonnet and haiku
seats, one family). The model names are taken from what the records report and written into the report. The daemon Council uses the
seats in the operator's configuration (architect and strategist on Claude, red-team on Gemini in E2). A2's foreign seat is the
same red-team family as the Council's, which is the point of A2 (it equalises family spread with B1).

## Question set (fixed by written rules, before any E3 outcome was looked at)

**Rule E (carried over).** The four questions that **qualified under E2's pre-registered rule**: q3, q4, q5, q6 (`dev/train/e2/data/analysis.json`
lists them as the four qualifying). Their question files are E2's, unchanged.

**Rule N (new questions).** From every `council_format: 1` record under `~/.delphi/workspaces/delphi/local/missions/*/councils/`,
`~/.delphi/workspaces/delphi/local/sessions/*/councils/` and the repository's `missions/*/councils/`, that is **not** one of E2's
eight, take them in order of the record's `timestamp`, newest first; keep a record only if its proposition paragraph is at most
2,000 characters (the `request_council` limit, so no question is split as in E2); take the first two. The question is the proposition
paragraph, with the stakes `normal` and one pointer to the spec the record says it is reviewing. Records were only **read** to extract
the text. The source Councils' rulings are **not** used as a reference. Applied on 2026-11-07, the pool was: no record under
`~/.delphi/.../local/` newer than E2's q8 (the E2 pool is exhausted), and three in the repository: Council 015 (2026-11-04T02:00Z),
Council 014 (2026-11-02T23:50Z) and Council 007 (2026-10-24T05:40Z). The first two are taken; 007 is not. (E2's PREREG said the
repository's records lacked `council_format: 1`; these three have it. They were outside E2's set because E2's rule looked only
under `~/.delphi`. Stated here so the discrepancy is visible.)

| id | origin | source (relative to `~/.delphi/workspaces/delphi/local/` for q3 to q6; to the repository root for n1, n2) | sha256 of the source record | sha256 of the question file |
| :-- | :-- | :-- | :-- | :-- |
| q3 | E2, qualified | `sessions/host-3d16647e8cd5bbfd/councils/001-a-host-agent-host-copilot-cli-unknown-wo.md` | `a5e6913f3338d6cbbffda7e4aad28f7dcc7b7ae1f339a820c8e39ea25ad410ec` | `72ccc85220b78c754a51931a720c6ca8c975bed80228dd060c4d48aab5fd49cb` |
| q4 | E2, qualified | `sessions/host-3d16647e8cd5bbfd/councils/002-a-host-agent-host-copilot-cli-claude-son.md` | `073bc87cc9c2d36af0d87c6662766d69d82e2b6e31f87d540231678ef578e447` | `4bb43686cf4c82ffed15e7e79c43aa968283400b2650bd67c0f1fbd70c070771` |
| q5 | E2, qualified | `sessions/host-da390f3c6d52f9b3/councils/001-a-host-agent-host-copilot-cli-claude-son.md` | `45ebee7407d04337f7b1a9e3f95622bfac457c289978fde6886da31d275599cf` | `5e12a2f3e3c254ed55f7ab03ad8ed851524e4020dea27c8f1f40dd6c3e5a8a29` |
| q6 | E2, qualified | `sessions/host-b7e82fd45591cf41/councils/001-a-host-agent-host-copilot-cli-unknown-wo.md` | `b9558c2332cd8668ba35062178afafcc6048dca6fad071c5d074f204648976be` | `c50c3050208bc1b47a3ac6b3e3a75535d6bad71864d59543650a4698f298e404` |
| n1 | Rule N, newest | `missions/2026-10-23-foundational-design/councils/015-spec-073-console-adaptive-contrast.md` | `67088fdd04852ef6995f26b87df5211f631badde4235e53987299bea046e04ae` | `1fe7ee36c1a1f32cf346dad9c1b618d7763eb8ffaa955e9c43e3b7b0bf47dc7f` |
| n2 | Rule N, second | `missions/2026-10-23-foundational-design/councils/014-spec-069-workspace-administration.md` | `938ed6495703e699616675ddceb69f963f2d9b11c24ffd2f0da6cbc0894af6d3` | `98ee19dcfeb932f88bf3afcfe94c56e21530bfe88078623d5f7d221d71c4179a` |

The q3 to q6 question files are `dev/train/e2/data/records/q3.txt` to `q6.txt` (their hashes are the ones in E2's notes, and above);
the n1 and n2 files are produced by `python3 -I dev/train/e3/score_e3.py make-question <source record>` and committed under
`dev/train/e3/questions/`. A mismatch of any hash at the dry run stops the experiment until the operator writes down what to do.

Notes fixed now: n1 and n2 ask whether a spec "should be approved"; both specs are now approved and built, so a seat that reads the
pointer sees that. This is a known weakness of the two new questions (threat 8) and is why they are listed as a different kind
from q3 to q6 (design questions with options).

A question is **complete** when it has B1, A1 and A2 (and B2 where scheduled).

## Seeds and repeats

Sampling cannot be seeded through the hosts or the daemon, so no seed is chosen. Run order is fixed in the RUNBOOK: phase 1, per
question in the order q3, q4, q5, q6, n1, n2: B1, A1, A2; phase 2: B2 on q3, q4, q5, q6. Each run is a fresh session. No run is
repeated to replace a result; failures are handled by the stop rules only.

## Measures (fixed; computed by `score_e3.py`, no model judges anything)

All are computed from a record's `council_format: 1` sections, parsed by E2's `score.py` (v1, imported unchanged): a heading
counts only when it starts a line and the last such line wins, so a heading forged inside quoted seat text or the question never
counts. The lines used are the `## Round 2 structured output` lines `- <seat> | recommendation | <label>`,
`- <seat> | unreadable | ...`, `- <seat> | P0|P1|P2|P3 | <title>`, and the `**Escalated to the operator:**` line of the Verdict section.

**Usable record.** A record is usable if it parses as `council_format: 1` with a Round 2 structured-output section, and (a Panel)
carries `blind: yes`. An `unreadable` seat does **not** make a record unusable in E3: it is the readable-critique measure. A B1 whose
decision class is `NONE` (no readable recommendation at all) is unusable as a reference and its question is dropped for every arm.

**M1, same decision (the class).** Each readable recommendation maps to a class: `PROCEED` and `PROCEED-WITH-CHANGES` to GO,
`DO-NOT-PROCEED` to STOP, `ALTERNATIVE: <text>` to ALT. The record's class is the one with strictly the most seats; a tie is SPLIT,
no readable recommendation is NONE. Two records have the **same decision** when both are GO, or both STOP, or both ALT and some
pair of their ALT texts has word-Jaccard at least 0.5 (E2's `same_decision`, unchanged, including E2's `STOPWORDS` and
normal form). SPLIT and NONE equal nothing. Counted per arm against B1, and for B2 against B1 as the noise floor.

**M2, readable-critique rate.** For a record, a seat is *readable* if it has a `recommendation` line in the structured output and
*unreadable* if it has an `unreadable` line and no `recommendation` line. A seat that abstained has neither and is not in the
denominator (reported separately). The rate of an arm is `readable seats / (readable + unreadable seats)` pooled over the arm's
usable records. The **Council rate** pools B1 and B2 (all usable records).

**M3, escalation rate.** The share of an arm's usable records that have a `**Escalated to the operator:**` line in the Verdict
section. The Council rate pools B1 and B2.

**M4, P0/P1 presence.** A record *raises* a blocking finding if it has at least one readable `P0` or `P1` line. Per question and
arm, `missed` = B1 raises one and the arm does not. (`added` = the reverse, reported only.) Findings are not compared with each
other; only presence is.

**M5, tokens.** A token is `input + output + thinking` of an `agent.usage` event (cache reads and writes reported, not added).
Daemon Councils and the foreign seat: `score_e3.py tokens`, summed by `task_id` with no `mode` filter; these are the
**Delphi-side** tokens. Panel host tokens (`claude -p` `modelUsage`, input + output over all models including subagents) are
**host-side** tokens, reported separately. A Panel's Delphi-side events must be 0 for A1 and non-zero only for the foreign seat in A2.
Tokens decide nothing in the rule except through the caps and stop rules.

### Constants (the test checks that `score_e3.py` carries these exact values)

```
MIN_QUALIFYING = 5
MIN_NOISE_QUESTIONS = 3
SD_ABSOLUTE_SLACK = 2
SD_RELATIVE_OK = 1
SD_RELATIVE_WORSE = 2
READABLE_WORSE = 1/4
ESCALATION_OK = 1/4
ESCALATION_WORSE = 1/2
MISSED_OK = 1
MISSED_WORSE = 3
CAP_PER_RUN = 350000
CAP_DELPHI_TOTAL = 1500000
CAP_HOST_TOTAL = 400000
```

## Decision rule (mechanical; `score_e3.py analyze` prints it)

Applied separately to A1 and A2. **A1 is the headline** (the Panel as shipped, comparable with E2); A2 is reported beside it, never
replaces it, and says only what the foreign seat changed. Let, for an arm X:

- `Q` = the questions where X's record, B1's record are usable and B1's class is not NONE (the arm's *qualifying* questions);
- `S` = the questions in `Q` that also have a usable B2 (at most four: q3 to q6);
- `sd` = questions in `Q` where X and B1 have the same decision; `sd_s` = the same, in `S` only; `sd_b2_s` = questions in `S` where B2 and B1
  have the same decision;
- `read_x`, `read_c` = M2 for X and for the Council; `esc_x`, `esc_c` = M3; `missed` = M4 over `Q`.

- **INCOMPLETE:** `|Q| < 5`, or `|S| < 3`, or a rate has no denominator. No verdict for that arm. A run that failed twice (stop rules) is
  not in `Q`, so enough failures end in INCOMPLETE, never in a verdict from fewer questions.
- **WORSE** (any one): `sd_s <= sd_b2_s - 2`; or `read_x <= read_c - 1/4`; or `esc_x - esc_c >= 1/2`; or `missed >= 3`.
- **SUPPORTED** (all): `sd >= |Q| - 2`; and `sd_s >= sd_b2_s - 1`; and `read_x >= read_c`; and `|esc_x - esc_c| <= 1/4`; and `missed <= 1`.
- **NO EVIDENCE:** anything else. This is not evidence that the Panel is as good, nor that it is worse.

WORSE is checked first, so an arm that meets one WORSE condition is WORSE even if it meets the SUPPORTED ones.

### E2 baseline for comparison (not part of the rule)

Same measures on E2's final records, all eight questions, Claude Code Panel against the unfixed build, from `dev/train/e2/data/records/`
(`python3 -I dev/train/e3/score_e3.py baseline-e2`; the machine-readable E2 verdict is `dev/train/e2/data/analysis.json`):

| | readable-critique rate | escalation rate | P0/P1 present |
| :-- | --: | --: | --: |
| E2 Panel (8 final records) | 21/24 = 0.875 | 8/8 = 1.000 | 8/8 |
| E2 B1 | 23/24 = 0.958 | 5/8 = 0.625 | 8/8 |
| E2 B2 | 23/23 = 1.000 | 6/8 = 0.750 | 8/8 |
| E2 Council pooled | 46/47 = 0.979 | 11/16 = 0.688 | 16/16 |

Same decision on the four E2-qualifying questions (`analysis.json`): Panel against B1 3/4, B2 against B1 3/4; the Panel said GO on 8 of 8
questions, including two (q2, q6) where both Councils said ALT. These E2 numbers came from a build with the Round 2 defect (a prose-only
red-team reply was kept as unreadable); the point of E3 is whether the fixes moved them.

## Budget (tokens, never dollars)

The operator approved, on 2026-11-07: a **per-run cap of 350,000 tokens**, a **total Delphi-side cap of 1,500,000 tokens** (daemon
Councils and the foreign seat), and a **host-side cap of 400,000 tokens** across the experiment for Panel host tokens (host-account
tokens, counted and reported separately). Caps are never raised after the first run; the operator may lower them in a commit before it.

**Measured costs from E2** (`dev/train/e2/data/tokens.csv`, `REPORT.md`): 21 daemon sessions, 1,918,650 tokens, an average of 91,364
including failed ones (largest 135,130); planned here at **91,600** per Council. 9 Panels, 175,916 host tokens, an average of 19,546
(largest 30,024); planned at **19,500** each. A foreign seat is two seat calls (Round 1 and Round 2) of a Council's seven model calls
(three seats times two rounds, plus the synthesis): about 26,000; planned at **30,000** each (not measured on its own; the live
cost is a stated unknown).

| item | count | planned each | Delphi-side | host-side |
| :-- | --: | --: | --: | --: |
| B1, all six questions | 6 | 91,600 | 549,600 | |
| B2, q3 to q6 only | 4 | 91,600 | 366,400 | |
| A2 foreign red-team seat | 6 | 30,000 | 180,000 | |
| A1 Panels | 6 | 19,500 | 0 | 117,000 |
| A2 Panels (host seats) | 6 | 19,500 | (counted above) | 117,000 |
| **Planned total** | | | **1,096,000** | **234,000** |

**Reserve.** 25% of the Delphi-side cap is held for re-runs under the stop rules: 375,000. 1,096,000 + 375,000 = **1,471,000**, under the
1,500,000 cap with 29,000 to spare; the reserve pays for about four re-run Councils (4 x 91,600 = 366,400). Host side: 234,000 planned,
166,000 under the cap, about eight more Panels at 19,500 (at E2's largest Panel, 30,024, the twelve Panels would cost 360,288).

**The design the operator first had in mind does not fit.** Eight questions with all four arms on every question would be 16 daemon
Councils (1,465,600) plus 8 foreign seats (240,000) = 1,705,600 Delphi-side, and 2,080,600 (about 2.1M) with the 25% reserve: it does not
fit 1,500,000, and the base cost alone exceeds it with any re-run. **What the chosen design drops:** two of the eight questions (the
four E2-qualifying ones are kept, plus two new ones); B2 on the two new questions (so the noise floor rests on four questions, and
the two new ones have no repeat-Council comparison); and a repeat Council or Panel per arm. It keeps both Panel arms on every question.

## Stop rules and exclusions

- **Caps.** Before any Council starts, the running Delphi-side total plus 135,130 (E2's largest run) must stay at or under 1,500,000, and the
  host-side total plus 30,024 at or under 400,000; otherwise no new run starts. A run that passes 350,000 on its own side is stopped, recorded
  `capped`, counted, and excluded from the rule. If the planned spend overruns 1,125,000 (the 1,500,000 cap less the reserve) before phase 2,
  the B2 runs not yet done are dropped, **last question first**, and the report says so; the INCOMPLETE rule then decides.
- **Daemon seat failure.** The P4b retry is automatic: an `agy` seat that ends on the CLI's own error, before using a tool, with at least
  10 seconds left, is re-run once inside the same run (event `harness.retry`; both attempts' tokens count). Only if the run still ends with a
  failed or abstaining seat because of a harness failure (the error log shows `harness_failed`, or the record shows an `abstained` seat caused by one) is
  the whole run **re-run once** in a new session, in phase 2 or after its phase. A second failure **stands and is scored as it is**: an
  `unreadable` red-team counts against the Council's readable rate, exactly as it would against a Panel's. (E2 excluded such runs; here that
  would make the Council look better than it is.) The first attempt's record is kept and reported; the rule uses the final attempt.
- **Quota or exhaustion** (a `harness_exhausted` log entry, a usage-limit message from the host, a rate limit that persists after one retry)
  **ends the experiment INCOMPLETE** at that point. Nothing is waited out and resumed.
- **Panel infrastructure failure** (host crash, timeout, no record written): re-run once in a new session; a second failure excludes it. A Panel whose
  moderator rewords the question (`score.py question-check` false) is excluded and re-run once. A Panel red-team that ends in prose is **not** re-run:
  it is the M2 measure.
- **A host that cannot run isolated seats** is excluded and reported. A headless Panel that runs without isolated subagents, or whose record is
  not `blind: yes`, is **excluded, never scored as blind**.
- **No cherry-picking.** No question is added or removed after the first run; no run is repeated or replaced because its result is disliked;
  the analysis is run once, after every run that will be made is done.

## What each outcome changes

Only the wording and trigger list of the section 4.5 notice that `council_frame` shows. **Never** the Panel default, a block, a second
confirmation, a refusal, a trust level, or the operator's freedom to run a Panel for any question. Nothing about finding-level quality is
claimed in any outcome.

- **SUPPORTED (A1):** the notice's comparative suggestion line may be softened for reversible, in-project triggers; the authority, trust, sandbox,
  irreversible and "formal" triggers stay. Reported with the sentence "agreed in class and rate on six questions on one host".
- **NO EVIDENCE:** the notice is unchanged.
- **WORSE (A1):** the notice states the measured gap in plain counts and is shown on every Panel, still non-blocking.
- **INCOMPLETE:** the notice is unchanged and E3 is reported as not completed.
- **A2 against A1** changes nothing by itself; if A2 is SUPPORTED and A1 is not, the report says so and the notice may mention that a foreign seat
  was in the better-agreeing arm, as a statement of the result and not a recommendation to turn it on.

Any such change is its own pull request under its own spec text.

## Power, honestly

Six questions on one host, one run of each Panel arm, four noise-floor repeats. One question flipping moves `sd` by 17 points (25 within the
four-question subset). M4 has almost no power: in E2 every arm raised a P0/P1 on 8 of 8 questions, so `missed` is expected to be 0 for
everyone; it can catch a collapse, not a difference. M2 and M3 are counts of seats and records (about 15 to 18 seats per arm, six records); a
single unreadable seat moves the readable rate by about five points. The reference B1 is itself a model output, and the noise floor rests on
four B2 runs. Two of the six questions (n1, n2) are not design questions with options. Thresholds were chosen before any E3 data to keep a
small or noisy difference from moving the notice; they are decision thresholds, not significance tests, and have no tuned power. The SUPPORTED
bar is deliberately hard to meet on six questions; NO EVIDENCE is the expected outcome of an honest small experiment.

## Explicitly NOT being measured

Seat accuracy or correctness (there is no ground truth; "the same as the Council" is agreement); finding-level overlap or recall (dropped,
matcher v2 failed validation); prose quality, usefulness, latency, the ruling card; whether the Panel is "as good as a Council" in general.
E3's numbers are never shown to a seat, put in a prompt, used to rank models or seats, or kept as a leaderboard (spec 078 section 3).

## Threats to validity

1. **Same decision is coarse.** GO against GO includes "proceed with changes" on both sides; a Panel and a Council can agree on the class and
   differ on everything that matters. The experiment measures the class, not the content.
2. **The reference is a model output** and differs from itself (B2). With four repeats the noise estimate is itself rough.
3. **A GO prior.** The Panel said GO on 8 of 8 in E2, and n1, n2 are "should this spec be approved" questions about specs that are now
   approved and built. Agreement on GO can be agreement on the easy answer.
4. **Contamination.** The repository checkout the seats read contains the source Council records for n1, n2 (and 007) and the approved specs.
   Mitigation in the RUNBOOK: the run checkout is a throwaway worktree with those three records deleted; the specs and the tracker remain
   and can reveal the outcome. For q3 to q6 the scratch homes hold none of E2's sessions; paraphrases cannot be ruled out.
5. **Family spread.** A1 is single-family and B1 is cross-family; A2 equalises the red-team family but its foreign seat is Delphi-run, so A2 is
   not purely a host Panel. The Round 2 shape check (P1) applies to host seats only; the foreign seat is not re-asked.
6. **`agy` reliability.** The foreign red-team seat (A2) and the Council's red-team seat run on `agy`/Gemini, which failed in 7 of 21 E2 sessions.
   P4b's retry is built and unit-tested but its effect on a live run is unmeasured; failures that stand are scored (stop rules), so they
   count against the Council's and A2's readable rates, not against A1.
7. **Isolation is host-reported.** A moderator that pastes stances between seats defeats `blind: yes` and the server cannot see it. The label
   is a claim. The confirmation and options-source fields (P3) are host-reported too.
8. **Different question kinds.** q3 to q6 are host-raised design questions with options; n1, n2 are single yes/no approval questions; the
   rule is applied to all six pooled. The kinds are listed per question in the report.
9. **Authors as scorers.** No model scores; the rules were written by an agent of the headline host's family before any data. The E2 lesson
   that a crude matcher can silently zero a measure applies: `score_e3.py` is tested on fixtures for each measure, and a measure that is
   identically the same for all arms (M4) is reported as not discriminating.
10. **Small n, one date, one daemon configuration, one host.** Any result is about this set and this build.

## How it is run

See `RUNBOOK.md`. Scoring: `python3 -I dev/train/e3/score_e3.py`, tested by `python3 -I dev/train/e3/test_score_e3.py`. This file is committed
first and alone; `score_e3.py`, its tests, the n1 and n2 question files and the runbook follow in a second commit, both before any run. A later
change to `score_e3.py` that alters any definition above voids the experiment; a bug fix that does not is allowed before the first run and is
noted in the report.
