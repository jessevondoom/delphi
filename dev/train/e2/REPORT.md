# E2 report: Panel vs daemon Council (spec 086 S5)

> **Provenance.** This is the text of the description of PR #514, written by the agent that ran the experiment on 2026-11-06, saved here unchanged except for this header, the removal of the PR footer and one sentence about the file being missing. Before saving it, the person committing it checked the headline figures against the committed data: the verdict (`INCOMPLETE`), the rule inputs (4 qualifying questions; same decision 3/4 for both arms; pooled overlap and recall 0.0; 40 reference findings; `p0_missed` 3; findings added 52 and 41) against `data/analysis.json`, and the token totals (daemon 1,918,650 + 2,043 aborted = 1,920,693; Panel 175,916; total 2,096,609; largest run 135,130) against `data/tokens.csv`. The matched and unmatched finding pairs, the failure causes and the per-question table were not independently re-derived. The experiment's design is `PREREG.md`; the steps are `RUNBOOK.md`; every deviation is in `data/notes.md`.
>
> **What to take from it.** It is *not* evidence about whether the Panel is as good as a Council: the verdict is INCOMPLETE and the finding matcher could not match even a Council against a second run of itself. It *is* evidence about defects: see "What this changes" and `specs/089-panel-hardening-from-e2.md` (the fix plan).

## Verdict: INCOMPLETE (pre-registered rule, unchanged)

Only 4 of 8 questions qualify (q3 to q6); the rule needs 6. No verdict about the Panel. Per PREREG "What each outcome changes", the section 4.5 notice is unchanged. The Panel stays the default and allowed for every question.

Operator go (2026-11-06): "go on s5 using a 350k token cap". PREREG Amendment 1 (committed before any model call) lowers the per-run cap 400,000 to 350,000; total 8,000,000 unchanged; Claude Code is the only host. Built from main 7af77d6 plus the amendment commit.

Data (analysis.json from `score.py analyze`, run once; tokens.csv; notes.md with every deviation; all records and the eight question files) is in `dev/train/e2/data/`. This file is that report.

## Counts

Rule inputs on the 4 qualifying questions: sd_panel 3/4, sd_council_repeat 3/4, pooled overlap 0.0 (Panel) and 0.0 (Council repeat), pooled recall 0.0 and 0.0 (0 of 40 reference findings matched by either), p0_missed 3, panel "added" 52, council-repeat "added" 41. These decide nothing (q < 6).

| q | A | B1 | B2 | A=B1 | B2=B1 | findings A/B1/B2 | degraded | qualifies |
| :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- |
| q1 | GO | GO | GO | yes | yes | 4/7/3 | A red-team R2 unreadable; B2 red-team abstained twice (excluded) | no |
| q2 | GO | ALT | ALT | no | no | 16/7/10 | B1 red-team R2 unreadable, twice | no |
| q3 | GO | GO | ALT | yes | no | 12/7/11 | none | yes |
| q4 | GO | GO | GO | yes | yes | 15/11/9 | none (after re-runs) | yes |
| q5 | GO | GO | GO | yes | yes | 18/11/10 | none | yes |
| q6 | GO | ALT | ALT | no | yes | 7/11/11 | none | yes |
| q7 | GO | GO | GO | yes | yes | 4/10/12 | A red-team R2 unreadable | no |
| q8 | GO | GO | GO | yes | yes | 8/9/6 | A red-team R2 unreadable | no |

Panel escalated in 8 of 8; B1 in 5 of 8; B2 in 6 of 8. Panel recommended GO in all 8, including q2 and q6 where B1 and B2 both said ALT.

## Runs, exclusions, re-runs

- Arm B: daemon Council through an MCP client script; 21 sessions (8 B1, 8 B2 first attempts, plus 5 re-runs). The red-team seat runs on agy/Gemini and failed in 7 of 21 sessions: "the CLI reported an error" after a 20-25 minute hang (q1 B2 twice, q2 B1 twice, q4 B1) or "the agy run changed a project checkout; its output is discarded" (q4 B2, q7 B1). Per the stop rule each was re-run once in a new session (out of the B1, A, B2 order). q4 B1, q4 B2 and q7 B1 recovered; q1 B2 and q2 B1 failed twice and are excluded.
- Arm A: real headless `claude -p` 2.1.319 with a copy of the plugin directory (its `.mcp.json` running the built binary on a scratch home), `--strict-mcp-config`, `--setting-sources project`, `--model sonnet`, no `--dangerously-skip-permissions`. Six subagent Agent calls per Panel (3 seats x 2 rounds; q5 nine), seats submitted themselves except q1 (moderator submitted the strategist's Round 1) and q5 (moderator submitted two Round 1 stances). All final Panel records say `venue: panel`, `blind: yes`, and contain their question verbatim. q1 attempt 1 was excluded and re-run once because the moderator reworded the question and invented options.
- The Panel red-team seat's Round 2 came back `unreadable` in q1, q7 and q8 (it ended in prose, no `recommendation` line). These are not infrastructure failures and were not re-run. The card says "no readable critique" and the Panel escalates.
- The host asked the cost confirmation only in q1 (attempt 2), q2 and q6; I answered "yes" by resuming the session. In q3, q4, q5, q7 and q8 it took the prompt as the yes.

## Tokens (input + output + thinking; cache excluded, as PREREG defines)

| item | tokens |
| :-- | --: |
| Arm B, 21 daemon sessions | 1,918,650 |
| Aborted start of q2 B1 (my tool stopped it; no record) | 2,043 |
| Arm A, 9 `claude -p` runs (host `modelUsage`, all subagents) | 175,916 |
| **Total** | **2,096,609** of 8,000,000 |

Largest single run 135,130 (q2 B1 attempt 1) of 350,000; no run capped; no NOT MEASURED rows. Panel cache reads were about 8.1M tokens in all (0.5 to 1.4M per Panel), not counted by PREREG's definition; "a Panel costs a seventh of a Council" holds for fresh tokens only. Panel tokens are on the host's account, a Council's on Delphi's.

## Models as the records reported them

Daemon: `sonnet` (architect, strategist, orchestrator) and `gemini-3.8-flash-medium` (red-team). Panel (host-reported): architect `claude-opus-5-5` (q6 `claude-sonnet-5-5`), red-team `claude-sonnet-5-5` (q6 `claude-opus-5-5`), strategist `claude-haiku-5-5` (a few seats `claude-sonnet-5-5`), moderator `claude-sonnet-5-5`. One family, several models.

## What the numbers can and cannot say

The word-overlap rule at 0.5 Jaccard is too crude: a Council against a second run of itself also matched 0 of the findings in 4 of 4 qualifying questions. Pairs that a reader would pair but the rule did not (verbatim, q6 B2 vs B1, 0.45): "TOCTOU and corruption risks in external host config mutation (D-2)" / "Host config mutation creates external state corruption and privilege escalation (D-2)". And (0.35): "D-3 launcher script is executable content run unsandboxed by the host; no justification why a plain command entry in MCP config is insufficient; defer" / "D-3 launcher script ships executable content run unsandboxed by the host (§5.5); a bare `command: delphi` needs no script, so cost exceeds return". Matched (0.58, q1 B2 vs B1): "Critic must be a different model family than the drafter (H-008); label the verdict homogeneous if only one family is configured" / "Reviewer must be a different model family than the drafter (H-008); label homogeneous if only one family exists". So `p0_missed` 3 and the 0.0 overlaps carry no information about the Panel.

## Venue cross-checks

Daemon records have no `venue:` line (`venue_missing`). Every Panel record says `venue: panel` but `score.py` expects `host`, so it prints `venue_mismatch` for all; this is a constant in the scorer that disagrees with what S4 writes (a cross-check only; the rule is unaffected). `score.py` was not changed. Recommend a separate fix.

## Threats (PREREG)

1 (same-family Panel vs cross-family Council): unchanged; the cross-family Gemini seat was the main source of daemon failures. 2 (reference is a model output): confirmed far beyond expectation. 3 (record bug history): the unreadable Round 2 had two new causes: the Gemini CLI error (daemon) and the Panel red-team seat ending in prose (Panel). 4 (scorer crude): see above. 5 to 9: unchanged; n is now 4 qualifying questions. The scratch homes lack the operator's memory and KB, equally for both arms.

## What this changes

INCOMPLETE: the notice is unchanged. Observations that bear on notice wording (each needs its own PR and spec text): a Panel's red-team critique can be unreadable (4 of 9 Panel runs counting the first q1 attempt) and the Panel then escalates; the moderator can reword the question (q1 attempt 1); in 5 of 8 final runs the host did not ask the operator for the cost confirmation.

## Deviations from the RUNBOOK

1. Arm B through an MCP client script (no host model), splitting the question file into the tool's fields (request_council rejects a question over 2,000 characters; q8's file is 2,303). 2. Arm A as headless `claude -p` with the options above; the "yes" by session resume. 3. Re-runs after the first pass, out of order. 4. An aborted q2 B1 start (2,043 tokens), daemon restarted. 5. Fresh scratch homes with copied real config and ratified files (removed afterwards). 6. The printed ruling cards are not committed (they contain the orchestrator's theme name, which the lint forbids outside themes/); the records carry `venue:` and `blind:`. 7. `REPORT.md` is not committed (see top).

Gates run: `python3 -I dev/train/e2/test_score.py` (19 tests OK), `delphi lint --repo .` (0 fail), `cargo test -q --locked -p delphi --test docs --test spec_refs` (ok). Not run: the full `cargo test --workspace` and clippy (no Rust code changed).
