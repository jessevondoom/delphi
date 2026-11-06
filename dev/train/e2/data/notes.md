# E2 run notes (spec 086 S5)

Run 2026-11-06. Branch spec-086-s5-run, based on origin/main 7af77d6 plus the PREREG Amendment 1 commit 5309dda (built binary = HEAD 5309dda;
the only change over main is PREREG.md). Binary: target/debug/delphi of the worktree, built `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build -q --locked -p delphi`.

## Authority and caps

Operator go 2026-11-06: "go on s5 using a 350k token cap". Per-run cap 350,000 (PREREG Amendment 1), total 8,000,000, headline host Claude Code only.

## Dry run (no tokens)

- test_score.py: 19 tests OK.
- Source record sha256: all eight match PREREG's table.
- Questions extracted with score.py extract-question; sha256 of q/q*.txt:
  q1 c3434e26..., q2 70debdea..., q3 72ccc852..., q4 4bb43686..., q5 5e12a2f3..., q6 c50c3050..., q7 3933918e..., q8 a78f6055...
- Pointers: every `specs/`, `crates/`, `docs/`, `GOVERNANCE.md` pointer in the eight files exists in the checkout (none missing). q8's text also cites
  `~/Desktop/delphi-context-engineering-council-2026-11-05.md` in prose (not a pointer; not provided to either arm).
- Source Council cost (score.py tokens on the source events, by task_id of the council.verdict event): q1 7,130 (mission 2026-10-26, Opus/Gemini, 6 events);
  q2 123,424; q3 105,926; q4 111,483; q5 133,914; q6 157,026; q7 116,672; q8 111,916 (all 7 events, sonnet + gemini-3.8-flash-medium).
  Sum of eight about 867k; 16 daemon Councils at ~120k estimate about 1.9M, within 8M. The largest source Council (157k) is under the 350k per-run cap.
- Scratch homes: /tmp/e2-s5/home-council and /tmp/e2-s5/home-panel, each made with `delphi init --non-interactive --workspace delphi --home H`
  plus copies (read only from the real workspace) of config.toml, capabilities.json, local/endpoint-capabilities.json, local/ratified/ and
  ratified.lock (without them every seat abstains with no_model_configured). Being fresh, the homes contain none of the source Councils or
  sessions (the isolation PREREG wants) and ALSO none of the operator's memory, KB or lessons, equally for both arms. The `grep -rIl -F` of the first
  60 characters of each question over both homes: 0 hits for all eight.
- Skill text: the generated `delphi-council` SKILL.md (release 0.5.5) tells the host to start one isolated subagent per seat, never to paste a peer's
  stance in round 1 and to say `blind: no` if it cannot isolate. Read, not run.
- Disk: 182 GiB free.

## Deviations from RUNBOOK (all recorded)

1. Arm B runs through an MCP client script (`council_run.py`, adapted from a script that speaks MCP to `delphi mcp`), not a host session. No host model is
   involved. The script splits the question file into `request_council` arguments (question text after "Question: ", stakes, options, pointers
   with reasons), because the tool takes fields, not a block; the daemon rebuilds the same fenced block. For q1 and q7 (operator-typed, no Question:
   prefix) the whole text is the `question`, so the daemon frames them as host-raised questions too (the fence "A host agent ... asks the Council").
   It passes `model: claude-sonnet-5-5` (the tool's self-reported field) and nothing else.
2. Arm A is a real headless Claude Code session (`claude -p`, 2.1.319) with the plugin directory copied from `ecosystem/claude-code/delphi` into
   /tmp/e2-s5/plugin-delphi (release 0.5.5 skill) loaded via `--plugin-dir`; the MCP server comes from `--strict-mcp-config --mcp-config` naming the same
   `delphi` server run as the built binary with `--home /tmp/e2-s5/home-panel`; `--setting-sources project` (the operator's user-level plugin is not loaded;
   verified in the init message: one plugin `delphi@inline`); `--model sonnet`; `--permission-mode default`; allowed tools: `mcp__delphi`, Agent, Task, Read, Grep, Glob,
   Skill. No `--dangerously-skip-permissions`. Script: /tmp/e2-s5/arm-a.sh.
3. One tiny smoke call of that host before any run (prompt "Reply with the single word ok", one turn, no tools; 25,730 cache-creation tokens, 2 input, 4 output):
   not part of any run, host-side, not counted per run. It showed: mcp server `delphi` connected, tools `mcp__delphi__council_*`, skills `delphi:delphi-council`,
   agents `delphi:architect|red-team|strategist|builder`, model `claude-sonnet-5-5`.

## Runs

Clock: US Eastern, 2026-11-06. Tokens are `input + output + thinking` for the daemon (score.py tokens, per session events dir). For the Panel (arm A)
the host's own `result.modelUsage` (all models incl. subagents, input + output; thinking is a subset of output; cache excluded) of the `claude -p`
session(s) is used, and the panel home's `agent.usage` events must show 0 (checked: `{}` for every Panel session).

### q1
- B1 (council t_0001, session host-8982dcd7ad7dfcd8): decided in 150 s, 106,736 tokens (sonnet + gemini-3.8-flash-medium), question-check true.
- A attempt 1 (session 4d1dabca, panel session host-aa5a6d99af93d92a): the host did NOT ask for the cost confirmation (took the naming of a Panel as the yes) and its moderator
  REWORDED the question (invented options, stakes low, "I'd like to have this system (Delphi) write its own README... the coding assistant (me)"). question-check false, so per
  RUNBOOK the run is excluded and re-run once. Cost: 15,053 tokens (80 in + 14,973 out; sonnet, opus, haiku). Kept as records/q1-panel-attempt1*.
  Note headless `claude -p` sees the seats run as background subagents and the session emits several `result` events; it completed the whole Panel in one invocation.
- A attempt 2 (re-run, session a85d78b3, panel session host-af9bf9ee13f035bd): turn 1 asked the cost confirmation (787 tokens) and ended; turn 2 resumed the same
  session with the single word "yes" (`claude -p yes --resume`; 14,197 tokens) and the Panel ran: three isolated subagents per round (Agent tool calls with parent ids, issued
  back to back, i.e. parallel), seats submitted themselves except the strategist, whose stance the moderator submitted on its behalf (the seat never called
  council_submit; moderator disclosed it). Models by seat as submitted: architect opus-5-5, red-team sonnet-5-5, strategist haiku-5-5 (the plugin agents' own model
  settings), moderator sonnet. Record has `venue: panel`, `blind: yes`. question-check true. Total 14,984 tokens. Red-team Round 2 UNREADABLE (its prose ended
  "Verdict: proceed" with no `recommendation` line), so the record escalated and is DEGRADED (left out of the rule's counts).
- B2 attempt 1 (session host-9b3e0fd087e1ab73): the red-team seat (agy, Gemini) hung about 20 minutes, then "the CLI reported an error" in Round 2: unreadable. 1437 s,
  99,962 tokens, needs_operator. Treated as infrastructure failure (timeout and CLI error) and re-run once.
- B2 attempt 2 (session host-e1fb244752497cde): the red-team seat abstained in Round 1 ("the CLI reported an error"). 1007 s, 9,112 tokens, decided on two seats.
  Second infrastructure failure: B2 for q1 is excluded (PREREG stop rule), degraded in the scorer.

### Incident
My first launch of the q2 chain used a tool-side background task that I then stopped (I feared a 10 minute kill); that aborted the q2 B1 start after ~2,043 tokens
(session host-6456fe70295d01c0, never completed, no record). I restarted the council daemon (new pid 38287) so no orphan Council kept running, and relaunched q2 B1 from scratch
with nohup. The 2,043 tokens are counted in the budget; the aborted start is not a run and is not re-run twice-counted.
