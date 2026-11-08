# E3 report: Panel vs daemon Council after the fixes (spec 089 P6)

> **Provenance.** This is the text of the description of PR #543, written by the agent that ran E3 on 2026-11-07, saved here unchanged except for this header, the removal of the PR footer and one sentence about the file being missing. Before saving it, the person committing it checked the headline figures against the committed data: the verdicts (A1 `NO EVIDENCE`, A2 `INCOMPLETE`), the rule inputs (A1: 6 qualifying, same decision 5, readable rate 1.0, escalation 0.1667 against the Council's 0.8333, 0 missed; A2: 2 qualifying) against `data/analysis.json`, and the token totals (Delphi-side 1,876,691; host-side 261,380; largest run 116,313) against `data/tokens.csv`. The per-question table, the failure causes of the lost A2 attempts, the re-run history and the model labels were not independently re-derived. The design is `PREREG.md` (with Amendment 1); the steps are `RUNBOOK.md`; every deviation is in `data/notes.md`.
>
> **What to take from it.** The Panel's readable-critique rate is fixed (P1). Its main difference from the Council is that it almost never escalates or raises a P0, which is consistent with an all-Claude Panel rarely dissenting; the pre-registered rule counts that against it (NO EVIDENCE) but does not measure finding quality. The foreign-seat arm mostly did not run: 8 of 10 attempts ended because a headless host stopped waiting for Delphi's seat (the skill text told hosts not to wait on it; spec 089 P7 fixes both the text and the server side). The `agy` retry (P4b) was never exercised, so nothing here credits it. A foreign seat costs about 90,000 Delphi tokens, three times the planned figure.

## Verdict

**A1 (Panel, Claude-only seats; the headline): NO EVIDENCE.**
**A2 (Panel with a Delphi-run foreign red-team seat): INCOMPLETE** (2 of 6 questions have a usable A2 record; the rule needs 5).

Operator go (2026-11-07): "replace the weak questions. you can raise the max budget to 2M tokens. then run e3". Built from `main` 86cf67c2 (PR #536; P1 #526, P2 #524, P3 #533, P4b #527 in the build). Claude Code 2.1.319 headless was the only host. This file is that report (the runner's file tool could not write it, so it was the PR description). Data: `dev/train/e3/data/` (`analysis.json`, `tokens.csv`, `notes.md` with every deviation, `manifest.json`, records as `*.md.txt`: byte-identical, renamed only because two Council records quote a seat's relative markdown link that fails the repo link lint). Nothing about finding-level quality is claimed.

A1 is not SUPPORTED because the escalation test fails: the Panel escalated on 1 of 6 records, the Council on 10 of 12 (gap 0.67, allowed 0.25). The other four SUPPORTED tests pass. It is not WORSE: the WORSE test is for a Panel that escalates more than the Council; here the sign is the other way.

## Counts

| | A1 (headline) | A2 |
| :-- | --: | --: |
| qualifying questions Q / with repeat Council S | 6 / 6 | 2 (q3, q5) / 2 |
| same decision as B1 (sd) / on S / B2 vs B1 noise floor | 5 of 6 / 5 / 5 | 1 of 2 / 1 / 1 |
| readable-critique rate (Panel / Council pooled) | 18/18 / 36/36 | 6/6 / 36/36 |
| escalation rate (Panel / Council pooled) | 1/6 = 0.17 / 10/12 = 0.83 | 1/2 / 0.83 |
| P0/P1 missed / added (M4, not discriminating) | 0 / 0 | 0 / 0 |
| verdict | NO EVIDENCE | INCOMPLETE |

## Per question (GO/STOP/ALT; esc = escalated)

| q | kind | B1 | B2 | A1 | A2 | esc B1/B2/A1/A2 |
| :-- | :-- | :-- | :-- | :-- | :-- | :-- |
| q1 | one sentence, no options (R2) | GO | GO | GO | excluded | no/no/no/- |
| q2 | long, stakes high (R2) | GO | GO | GO | excluded | yes/yes/yes/- |
| q3 | design | ALT | GO | GO | GO | yes/yes/no/no |
| q4 | design | GO | GO | GO | excluded | yes/yes/no/- |
| q5 | design | GO | GO | GO | GO | yes/yes/no/yes |
| q6 | design | GO | GO | GO | excluded | yes/yes/no/- |

q1 and q2 behaved like the rest on class (GO everywhere) and do not drive the verdict; the escalation gap is q3 to q6. Observation outside the rule: the Panel raised a P0 on 1 of 6 A1 records, the Council on 9 of 12. The Panel said GO on 6 of 6 (E2: 8 of 8).

E2 baseline beside E3: readable rate E2 Panel 21/24 = 0.875, E3 A1 18/18; escalation E2 Panel 8/8, E3 A1 1/6; Council pooled E2 0.979 / 0.688, E3 1.00 / 0.833.

## Exclusions, re-runs, deviations

- B1/B2: 12 Councils, each once, no failure, no re-run, none capped (largest 116,313 of 350,000). A1: 6 Panels, one each, all `blind: yes`, question verbatim, six isolated parallel subagent calls.
- **A2 (10 attempts, 2 records).** In 8 of 10 attempts the headless host asked for the foreign seat's Round 2, was refused the verdict brief ("waiting for red-team") two or three times and ended its turn, so `claude -p` exited before Delphi's seat answered: no record. Per PREREG: re-run once, second failure excludes. q1, q2, q4 excluded after two failures; q3 succeeded on the re-run; q5 on the first attempt; q6's re-run was not started because the pre-run cap check (1,876,691 + 135,130 over 2,000,000) forbids it. Prompt unchanged, no steering.
- **Order deviation:** after q1 and q2 A2 failed 4 of 4 (about 236k Delphi tokens), I ran B1 and A1 for q3 to q6, then all B2, then the remaining A2 runs, to keep failed-A2 spend from eating the 1,500,000 trigger that protects the noise floor. Only the order changed.
- Arm B through E2's MCP client script (daemons started with `delphi daemon restart`); host tokens are the last cumulative `result` event (a first script summed the cumulative events and was corrected before anything was recorded); foreign seat Round 2 usage NOT MEASURED in the two q1 attempts; cards, raw streams and scripts with absolute paths not committed.

## Tokens (cache excluded)

Delphi-side 1,876,691 of 2,000,000: B1 574,556, B2 534,955, A2 foreign seat 767,180 (10 attempts; the two finished ones 89,850 and 89,957; planned 30,000 each). Host-side 261,380 of 400,000: A1 116,806, A2 144,574. No quota error.

## Models

Daemon: `sonnet` (architect, strategist, orchestrator), `gemini-3.8-flash-medium` (red-team). Panel (host-reported): architect `claude-opus-5-5`, red-team `claude-sonnet-5-5`, strategist `claude-haiku-5-5`, moderator `claude-sonnet-5-5`; A2 red-team the Delphi-run `gemini-3.8-flash-medium`.

## Did the fixes help?

- P1 (shape check): yes. Panel red-team Round 2 readable in 18/18 A1 and 6/6 A2 (E2: 3 of 8 final Panels unreadable). The check bounced 6 Round 2 submits that the seats re-sent correctly.
- P4b (`agy` retry): the failure did not occur (no `harness_failed` anywhere; E2: 7 of 21 sessions), and `harness.retry` fired 0 times, so the retry was never exercised and the improvement cannot be credited to it.
- P3: `confirmation.needed` false for the five named Panels (q1, q3 to q6), true for q2 (stakes high) and every A2; no host asked the operator; no moderator reworded a question (E2: 1 of 9).
- Not fixed: a headless host cannot wait for the foreign seat (the skill says not to wait on it; the verdict brief is refused until it answers).

## What the verdict changes

NO EVIDENCE (A1): the section 4.5 notice is unchanged; the Panel stays the default and allowed for every question. For whoever next edits the notice (separate PR, own spec text): the Panel almost never escalates because its seats rarely raise a P0; a headless host loses a Panel with a foreign seat (8 of 10 attempts); a foreign seat costs about 90,000 Delphi tokens, three times the planned figure.

## Threats

As PREREG: unchanged except 6 (`agy` reliability: no failures observed, 0 retries, one day and one machine), 8/12 (q1 and q2 did not depress agreement or readability), 13 (selection after the fact stands), and new: the A2 arm as run is not the arm as designed. Contamination mitigated per RUNBOOK (E2 results and E3 files deleted from the run checkout), not excluded.

## Gates

Run: `python3 -I dev/train/e3/test_score_e3.py` (26 OK), `cargo run -q --locked -p delphi -- lint --repo .` (0 fail), `cargo test -q --locked -p delphi --test docs --test spec_refs` (ok). No Rust changed; the full workspace test and clippy were not run. Scratch homes, daemons, caffeinate and the run checkout were removed.
