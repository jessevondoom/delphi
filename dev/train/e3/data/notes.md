# E3 run notes (spec 089 P6)

Run 2026-11-07, US Eastern. Branch `spec-089-e3-run`, based on `origin/main` 86cf67c2677673356684865daaa40965bf1620bb (= the built commit; merge of PR #536, the Amendment 1 / runbook / scorer PR). Binary: `/tmp/e3/checkout/target/debug/delphi` (delphi 0.5.5), built `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build -q --locked -p delphi` in a throwaway worktree `/tmp/e3/checkout` (detached at 86cf67c2). The installed `~/.local/bin/delphi` and the real `~/.delphi` were never written (a different, installed daemon was running on the real home and was left alone).

## Authority and caps

The operator wrote on 2026-11-07: "replace the weak questions. you can raise the max budget to 2M tokens. then run e3". PREREG Amendment 1 (PR #536, merged before any run) implements it. Authorised: per-run cap 350,000; Delphi-side total 2,000,000 (including the foreign seat); host-side Panel 400,000; Claude Code the only host; nothing else.

## Gate and dry run (no tokens)

- Prerequisites in `main` (git log): P1 #526 (fe809de4), P2 #524 (4df534cb), P4b #527 (b0cdb01c), P3 #533 (8958565a), amendment #536 (86cf67c2).
- Skill text: the generated `delphi-council` SKILL.md of the build carries the P3 wording (`requested_by`, `options_source` operator|host|none, `operator_confirmed: true` required with `foreign_seat`, "question is the operator's words"). Seats are to be one isolated subagent each; a peer's stance is never pasted.
- `python3 -I dev/train/e3/test_score_e3.py`: 26 tests OK. `python3 -I dev/train/e2/test_score.py`: 19 tests OK.
- Source records: all six sha256 match PREREG Amendment 1's table (q1 dfb0683c, q2 eeb9312a, q3 a5e6913f, q4 073bc87c, q5 45ebee74, q6 b9558c23).
- Question files copied byte-identical from `dev/train/e2/data/records/q{1..6}.txt` before deletion; sha256 match the table (c3434e26 q1, 70debdea q2, 72ccc852 q3, 4bb43686 q4, 5e12a2f3 q5, c50c3050 q6). Sizes 92, 830, 935, 1659, 723, 956 bytes (all under 2,000).
- Run checkout deletion (threat 11): in `/tmp/e3/checkout` removed `dev/train/e2/data/`, `dev/train/e2/REPORT.md`, `dev/train/e2/RESCORE.md`, `dev/train/e3/`. The specs, the tracker and other Council records remain. Every arm ran with this directory as its working directory.
- Pointers: every pointer in the six files exists in the checkout (q2: specs 062, 064, 041, 003; q3-q6: specs/GOVERNANCE/docs/crates paths); q1 has none.
- Scratch homes `/tmp/e3/home-council` and `/tmp/e3/home-panel`: `delphi init --non-interactive --workspace delphi --home H`, then read-only copies of `~/.delphi/workspaces/delphi/config.toml`, `~/.delphi/capabilities.json`, `local/endpoint-capabilities.json`, `local/ratified/` (cp -Rc) and `ratified.lock`. Only change: `[workspace] projects` in each copy's config.toml set to `["/tmp/e3/checkout"]` (was the operator's working checkout). `doctor` shows 2 endpoints configured (claude-sub, agy-sub); it reports `fail` on the ratified lock (two memory lesson files missing from the lock: a property of the copied live state; seats loaded fine) and warns probe results over 24 h old.
- The `grep -rIl -F` of the first 60 characters of each question over both homes: 0 hits for all six.
- Caffeinate: `caffeinate -dimsu -t 86400` (pid 55910) held for the whole run.

## Deviations from the RUNBOOK (all recorded)

1. Arm B through an MCP client script (`council_run.py`, E2's, copied unchanged into `data/`), not a host session. As in E2 it splits the question file into `request_council` fields (question text, stakes, options, pointers with reasons), because the tool takes fields. q1 has no `Question:` prefix so the whole text is the question. It passes `model: claude-sonnet-5-5` and nothing else. (The six files are all under 2,000 characters, so no field is truncated.)
2. The daemons were started with `delphi daemon restart --workspace delphi --home <scratch home>` (the MCP client does not start one: `daemon_not_running`); pids 59446 (council) and 59474 (panel).
3. `claude -p` 2.1.319 as the RUNBOOK says; the plugin copy's `.mcp.json` runs the built binary with `--workspace delphi --home /tmp/e3/home-panel`. Init message: plugin `delphi@inline` (plus builtin ones), MCP server `delphi` connected, `mcp__delphi__council_*` tools present, model claude-sonnet-5-5, the operator's user-level plugin not loaded.
4. Watchers (`watch.py`) poll the Delphi-side token sum once a minute and would SIGKILL the arm's daemon above 350,000; the host-side per-run total is read from the stream after the run (a Panel run is a single short `claude -p`).

5. **Host tokens are the LAST `result` event's cumulative `modelUsage`** (input + output over all models, cache excluded). A headless `claude -p` run emits several `result` events whose `modelUsage` is cumulative; my first summary script summed them (q2 A1 showed 159,130) and was corrected before anything was recorded (true value 25,732). All host figures in `tokens.csv` use the corrected rule.
6. **A2 deferral (order deviation).** The RUNBOOK order is B1, A1, A2 per question. After q1 and q2 every A2 run (4 of 4 attempts) ended with NO record: the headless host asked for the foreign red-team seat's Round 2, polled `council_status` two or three times, saw "waiting for red-team", and ended its turn with a message ("I can keep polling ... which do you want?"), so the session exited before Delphi's seat answered (the seat did finish about a minute later; the card was never written). PREREG: a Panel with no record is re-run once, a second failure is excluded; q1 and q2 A2 are therefore EXCLUDED. Because each failed A2 pair cost 58-179k Delphi-side tokens (the foreign seat is far above the 30,000 planned: 28-100k per attempt) and would eat the PREREG 1,500,000 trigger that protects the B2 noise floor, I ran the rest of phase 1 as B1 + A1 per question (q3 to q6), then phase 2 (B2), and left the q3 to q6 A2 runs for the end, each run once and, if no record, re-run once, as PREREG says, as long as the caps allow. This changes only the order, not any design, measure, question, cap or rule, and not the A2 prompt (no steering, no extra "keep polling" sentence, which would change the arm). Reported as a finding: a headless host cannot wait for the foreign seat.

## Runs

### q1 (kind: one-sentence operator-typed question, no options)
- B1 (council t_0001, session host-c96a1d1326f412e1): decided in 140 s, 93,253 Delphi-side tokens (sonnet + gemini-3.8-flash-medium), 0 harness.retry, question-check n/a (daemon). 3 seats answered and critiqued (architect, strategist: claude; red-team: gemini).
- A1 (claude -p, panel session host-e4d267cc981f745d): 6 Agent calls (architect, red-team, strategist, in two messages of three = parallel), seats called `council_brief`/`council_submit` themselves (parent tool use set), moderator submitted only the verdict; record `venue: panel`, `blind: yes`, question verbatim true; 14,332 host tokens, Delphi-side 0. The host did not ask for confirmation. Models: architect opus-5-5, red-team sonnet-5-5, strategist haiku-5-5, moderator sonnet-5-5. Red-team and strategist Round 2 had to be re-submitted once each (P1 shape check bounced a first submit).
- A2 attempt 1 (panel session host-ff83e03519580379): the architect and strategist ran as isolated subagents; the foreign red-team seat answered Round 1; the host polled council_status three times and stopped. No record. 10,010 host tokens, 28,280 Delphi-side (Round 2's usage never logged; recorded as NOT MEASURED for that span).
- A2 attempt 2 (panel session host-9e9117be43c2912a): same. No record. 8,697 host, 29,155 Delphi-side. EXCLUDED (second failure).

### q2 (kind: long host-raised question, stakes high, three options, four pointers)
- B1 (host-b1b8d4a8bc5fdf7a): decided, 104,838 Delphi-side tokens, 0 harness.retry; escalated to the operator.
- A1 (host-418fba824ee4ac58): record venue panel, blind yes, question verbatim true, 6 Agent calls (parallel in two messages of three); 25,732 host tokens; Delphi-side 0; the host did not ask for confirmation (the prompt had given it).
- A2: attempt 1 (host-bdea0b36e0c57973) 78,891 Delphi-side + 16,564 host tokens, no record; attempt 2 (host-d00145224875394d) 99,832 + 21,148, no record. EXCLUDED. Both: the host stopped while the foreign seat's Round 2 was outstanding.

### q3 (kind: host-raised design question with options)
- B1 (host-8e5dc086fd98c3a8): 100,497 Delphi-side tokens, 0 harness.retry, three seats answered and critiqued, escalated (5 unresolved P0).
- A1 (host-60b2b43b08b8952b): venue panel, blind yes, question verbatim true, 6 Agent calls (parallel pairs of three), 17,931 host tokens, Delphi-side 0.

### q4 (kind: host-raised design question with options)
- B1 (host-de65eecff177b808): 77,997 Delphi-side tokens, 0 harness.retry, three seats answered and critiqued, escalated (1 unresolved P0).
- A1 (host-bd0743c3b67ca7ad): venue panel, blind yes, question verbatim true, 6 Agent calls, 17,470 host tokens, Delphi-side 0.

### q5 (kind: host-raised design question with options)
- B1 (host-5ca664ba7253d208): 99,432 Delphi-side tokens, 0 harness.retry, three seats answered and critiqued, escalated (10 unresolved P0).
- A1 (host-bf180a4da501eb2c): venue panel, blind yes, question verbatim true, 6 Agent calls, 21,267 host tokens, Delphi-side 0.

### q6 (kind: host-raised design question with options)
- B1 (host-93b55d4903a295d6): 98,539 Delphi-side tokens, 0 harness.retry, three seats answered and critiqued, escalated (2 unresolved P0, conflicting Round 2).
- A1 (host-bccf4ac1976866fb): venue panel, blind yes, question verbatim true, 6 Agent calls, 20,074 host tokens, Delphi-side 0.

### Phase 1 total (before phase 2)
Delphi-side about 810k of 2,000,000; host-side about 173k of 400,000. Phase 2 (B2 on all six, in order) starts; the q3 to q6 A2 runs are deferred until after phase 2 (deviation 6).

### Phase 2 (B2)
- q1 B2 (host-171490cb89b0af34): 82,059 Delphi-side, 0 retry, 3 seats readable, not escalated.
- q2 B2 (host-79c58b1f3272015b): 92,973 Delphi-side, 0 retry, 3 seats readable, escalated (5 unresolved P0).
- q3 B2 (host-b99d5cdb269b422e): 77,930 Delphi-side, 0 retry, 3 seats readable, escalated (5 unresolved P0).
- q4 B2 (host-69d8827e84928491): 83,911 Delphi-side, 0 retry, 3 seats readable, escalated (Round 2 conflict).
- q5 B2 (host-f173e6960b87048d): 116,313 Delphi-side (largest run, under 350,000), 0 retry, 3 seats readable, escalated (10 unresolved P0).
- q6 B2 (host-9125946a3bbcf40a): 81,769 Delphi-side, 0 retry, 3 seats readable, escalated (1 unresolved P0, conflict).
- Phase 2 done: Delphi-side total 1,345,669 of 2,000,000 after all 12 Councils and the q1/q2 A2 attempts; host-side 173,225 of 400,000. The deferred A2 runs for q3 to q6 follow.
