# E3 runbook (operator-run)

**Amended by `PREREG.md` Amendment 1 (2026-11-07, before any run): questions q1 to q6, B2 on all six, Delphi-side cap 2,000,000.**
This runbook is the amended one. Writing `PREREG.md`, `score_e3.py`, its tests and this file spent no model request, and so does the dry
run in section 1. Every step from section 3 on calls a model and spends tokens (Delphi-side tokens for the daemon Councils and the
foreign seat, host-side tokens for the Panels). The design, measures, rule, caps and stop rules are in `PREREG.md` and are not
repeated or changed here.

Notation: `E3=/tmp/e3` (work directory, outside the repository), `SRC=~/.delphi/workspaces/delphi/local` (read only), `REPO=` the
repository checkout the binary is built from. Scratch runs always use `--home`; a bare `delphi` would touch the real workspace.
**Never use the installed `delphi` binary** (it may be an older release without the E2 fixes); build from `main` and call the build
by its path, `BIN=$REPO/target/debug/delphi`.

## 0. Gate before anything runs

1. **The operator's explicit, written go is on record.** On 2026-11-07 the operator wrote: "replace the weak questions. you can raise
   the max budget to 2M tokens. then run e3" (quoted in PREREG Amendment 1). The go is for the run **after** Amendment 1 and this runbook
   are committed and reviewed. The caps are 350,000 per run, 2,000,000 Delphi-side, 400,000 host-side, unchanged or lowered in a commit
   first, never raised. If the amendment or this runbook is changed after review in a way that alters PREREG, stop and ask.
2. **Prerequisites merged into the branch that is built** (`git log` shows each):
   - P1, the server-side Round 2 shape check: PR #526 (merged);
   - P2, the scorer and record `venue` fix: PR #524 (merged);
   - P4b, the one automatic `agy` retry and the path-naming discard message: PR #527 (merged);
   - P3, the skill wording, `operator_confirmed` and the options source: PR #533 (merged). Check the build has it: `council_open` with a
     `foreign_seat` refuses without `operator_confirmed: true`, and `council_frame` takes `requested_by` and `options_source`.
   - (P4a, #525, the diagnosis, and P5, #523, the matcher, are not prerequisites.)
3. `PREREG.md`, `score_e3.py`, `test_score_e3.py` and `questions/` are committed and unchanged since registration (`git status` clean for
   `dev/train/e3/`, and `git log -- dev/train/e3/PREREG.md` shows the pre-registration commit first).
4. **Keep the machine awake** for the whole run: `caffeinate -dimsu &` (note its pid; kill it at the end). E2's lesson: an idle sleep
   silently ruins a run. Plug in the power and disable the screen lock's sleep.
5. Disk: `df -h /tmp` shows room for two copies of the scratch home.

## 1. Dry run (spends nothing; stop on any failure)

```sh
python3 -I dev/train/e3/test_score_e3.py
python3 -I dev/train/e2/test_score.py
mkdir -p $E3/q $E3/records $E3/data
```

- [ ] Both test files pass (E3's also checks the constants, budget arithmetic and the question hashes against PREREG).
- [ ] **Source records unchanged.** `shasum -a 256` each of the six source records in PREREG Amendment 1's table (all under `$SRC`) and compare. A mismatch is reported, not fixed; the operator writes down what to do before the first run.
- [ ] **Question files.** `cp dev/train/e2/data/records/q{1,2,3,4,5,6}.txt $E3/q/` (E2's own files, byte-identical, never regenerated;
  `dev/train/e3/questions/` holds none);
  `shasum -a 256 $E3/q/*.txt` equals the last column of Amendment 1's table (`c3434e26...` q1, `70debdea...` q2, `72ccc852...` q3,
  `4bb43686...` q4, `5e12a2f3...` q5, `c50c3050...` q6). These bytes go to every arm unchanged. **Do this copy before the checkout step
  below deletes E2's data from the run checkout.**
- [ ] **A throwaway run checkout without the answers.** `git worktree add $E3/checkout <the built commit>`; in it delete E2's results and E3's
  own files so a seat cannot read them (PREREG Amendment 1, threat 11): `dev/train/e2/data/`, `dev/train/e2/REPORT.md`,
  `dev/train/e2/RESCORE.md`, `dev/train/e3/`; write the deletion into `$E3/data/notes.md`. Run every arm from this checkout (it is the "same
  repository commit"). Scoring commands run from the main repository checkout, not from this one. The specs and tracker remain.
- [ ] **Pointers exist** in `$E3/checkout`: `grep -hE '^- [^ ]+:' $E3/q/q*.txt`, then `ls` each (q1 has none). A missing pointer is listed in the
  notes and the question is not dropped.
- [ ] **Build the binary from `main`**: in `$E3/checkout`,
  `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build -q --locked -p delphi`; `BIN=$E3/checkout/target/debug/delphi`;
  `$BIN --version` printed in the notes.
- [ ] **Source Council cost for the caps** (reads only): `python3 -I dev/train/e3/score_e3.py tokens "$SRC/missions/<mission>/events" --task-id <t_NNNN>`
  for q3 to q6 (their `task_id`s are in E2's notes: `dev/train/e2/data/notes.md` lists the costs, 105,926 / 111,483 / 133,914 / 157,026; q1 and q2 are
  in the same notes). Compare with the 350,000 per-run cap. Planned: 12 Councils x 91,600 = 1,099,200, 6 foreign seats x 30,000 = 180,000,
  1,279,200 in all, plus the 500,000 reserve = 1,779,200 of the 2,000,000 cap.
- [ ] **Scratch homes (section 2)** made, then `grep -rIl -F "<first 60 characters of the question>" $E3/home-council $E3/home-panel` for each of
  the six questions (q1's whole sentence): zero hits expected; hits go in the notes.
- [ ] **Skill text.** Read the generated `delphi-council` skill: it must (a) tell the host to run one isolated subagent per seat, never paste a
  peer's stance into another seat; (b) say to pass the operator's question verbatim; (c) say that `council_frame` takes `requested_by` (`operator` when the operator asked for a Panel by name), that a Panel the operator named
  needs no second ask (`confirmation.needed: false`) **but** that a foreign seat or `high` stakes still make it `needed: true`, that
  `foreign_seat` needs `operator_confirmed: true`, and that `options_source` is `operator`, `host` or `none` (spec 089 P3). If (c) is not in
  the text, P3 is not in the build: stop.
- [ ] **Manifest skeleton** (`$E3/manifest.json`, section 5) lists the six ids; every path field is empty, not guessed.

## 2. Isolation and the scratch homes

Make **two** scratch homes so a Panel's staged record and a Council's memory never meet. Each is a fresh home built by the *built* binary,
plus a read-only copy of the operator's configuration. **Without the configuration files every seat abstains with `no_model_configured`**
(E2's lesson), so the copy list is not optional:

```sh
for H in home-council home-panel; do
  $BIN init --non-interactive --workspace delphi --home $E3/$H
  cp ~/.delphi/workspaces/delphi/config.toml                 $E3/$H/workspaces/delphi/config.toml
  cp ~/.delphi/capabilities.json                             $E3/$H/capabilities.json
  cp ~/.delphi/workspaces/delphi/local/endpoint-capabilities.json   $E3/$H/workspaces/delphi/local/endpoint-capabilities.json
  cp -Rc ~/.delphi/workspaces/delphi/local/ratified          $E3/$H/workspaces/delphi/local/ratified
  cp ~/.delphi/workspaces/delphi/local/ratified.lock         $E3/$H/workspaces/delphi/local/ratified.lock
done
```

(On the author's machine `config.toml` is at `~/.delphi/workspaces/delphi/config.toml`, `capabilities.json` at `~/.delphi/capabilities.json`, the rest
under `workspaces/delphi/local/`; check them with `ls` first. They are read, never written. Use `cp -Rc` for directories, never a symlink.) A fresh home contains none of the source
Councils and sessions, nor the operator's memory, KB or lessons, equally for both arms. Edit `[workspace] projects` in each copy's `config.toml` to list **only**
`$E3/checkout` so the `agy` checkout guard watches the throwaway tree and not the operator's working checkouts (it compares `git status`
of every registered project: any unrelated write there discards a good red-team answer, spec 089 section 13). Verify `$BIN doctor --home $E3/home-council`
shows the endpoints configured, and write what you changed into the notes.

Start the Council daemon for the experiment with `--home $E3/home-council` (the Council script or MCP client below starts or reuses it) and the
Panel daemon with `--home $E3/home-panel`. After every run the record stays in that arm's home.

## 3. Run (spends tokens; only after section 0)

**Order.** Phase 1, per question in the order q1, q2, q3, q4, q5, q6: **B1, then A1, then A2**, each in a **new session** (a new `delphi mcp`
connection, a new host process, a clean context). Phase 2: **B2 on all six, in the same order** (a complete noise floor). Before each run, check the caps (section 4). Do not skip,
reorder, add or replace a question.

### Arm B (B1, B2): the daemon Council

A fresh MCP client (as E2's `dev/train/e2/data/council_run.py`, which splits a question file into `request_council` fields: question,
stakes, options, pointers with reasons; read it, do not edit it) connected to `$BIN mcp --home $E3/home-council`. Send exactly the question
file's contents; pass nothing else. Poll `council_status` until done. Copy the record (`path` in `council_result`) to `$E3/records/<q>-b1.md`
(or `-b2.md`); note the `council_id` and the daemon's `task_id`. If a question file exceeds 2,000 characters the tool refuses it (none of the six
does). q1 has no options, stakes or pointers: `council_run.py` passes what the file has and its defaults for the rest, as E2 did; q2 has
stakes `high`.

### Arm A1 (Panel, Claude-only)

Headless Claude Code, one invocation per question, with the E2 setup that worked:

- **Plugin directory.** `cp -R ecosystem/claude-code/delphi $E3/plugin-delphi` (of the built commit), then replace its `.mcp.json` with
  `{"mcpServers":{"delphi":{"command":"<absolute $BIN>","args":["mcp","--home","<absolute $E3/home-panel>"]}}}`. The packaged
  `.mcp.json` runs the installed `delphi`, which must never be used.
- **Command** (one line; the allow-list is the whole permission set; **never** `--dangerously-skip-permissions`):
  `claude -p "<prompt>" --plugin-dir $E3/plugin-delphi --strict-mcp-config --mcp-config $E3/plugin-delphi/.mcp.json --setting-sources project
  --model sonnet --permission-mode default --allowed-tools mcp__delphi Agent Task Read Grep Glob Skill --output-format stream-json --verbose`,
  run with `$E3/checkout` as the working directory. Check the init message of the stream once: exactly one plugin (`delphi@inline`), the `delphi`
  MCP server connected, `mcp__delphi__council_*` tools present, and the operator's own user-level plugin **not** loaded.
- **Prompt** (fixed, the same for every question, then the question file's contents unchanged):
  `I, the operator, ask for a Panel by name (so requested_by is operator). Run it with Delphi's delphi-council skill, with isolated subagent seats (blind), on the following question, which is my own text: pass it verbatim, and say in options_source operator if it lists options and none if it does not. I have already confirmed the cost and the stakes, so do not ask me again. Print the council_result display unparaphrased.`
- **What the prompt must say, and why (merged P3 text).** (1) `requested_by: operator`: the operator asked for a Panel by name, so
  `confirmation.needed` is false and the host states its plan in one line and proceeds. (2) The question verbatim, and `options_source`
  `operator` when the file has an `Options:` line (q2 to q6) and `none` when it has none (q1); never `host`, since no options are invented.
  (3) The **confirmation is already given**, because q2 has stakes `high` and `council_frame` returns `needed: true` for `high` stakes even
  for a Panel the operator named; headless Claude cannot ask. The sentence is the operator's word that the cost and stakes are confirmed
  (true: the operator wrote the go quoted in section 0). If the host asks anyway, note it (a finding about P3, host-reported) and answer
  `yes` by resuming the session (`claude -p yes --resume <session>`); send nothing else, no steering.
- Copy the record to `$E3/records/<q>-a1.md` and the printed card to `$E3/records/<q>-a1.card.md` (not committed: it carries the orchestrator's
  theme name, which the lint forbids outside `themes/`). If the record has no `blind:` front-matter line, add `blind: yes|no` to the **copy**
  exactly as the card shows, and say so in the notes; add nothing else.

### Arm A2 (Panel with a foreign red-team seat)

Exactly A1, with the A1 prompt followed by one more sentence:
`Open it with council_frame and council_open foreign_seat: red-team and operator_confirmed: true: I, the operator, confirm that Delphi runs the red-team seat on another model family and spends its tokens.`
then the question file's contents. The merged P3 `council_open` refuses a foreign seat without `operator_confirmed: true`; that flag is only
ever sent because this prompt says it, and it is never defaulted by the harness. A foreign seat makes `confirmation.needed` true; the
prompt's confirmation answers it, so there is no second ask.
The foreign seat is Delphi-run, so the panel home's events carry its `agent.usage` (section 4).

### After each run (reads only)

- `python3 -I dev/train/e2/score.py question-check $E3/records/<q>-<arm>.md $E3/q/<q>.txt` must say `contains_question_verbatim: true`. If false the
  moderator reworded the question: that Panel run is excluded and re-run once (PREREG). Also check the card: `options: from the operator` (or
  absent) and no options the question file lacks.
- Panel records must say `venue: panel` and `blind: yes`; daemon records `venue: daemon` (`analysis` prints the cross-checks).
- **Isolation**: count the `Agent` calls in the stream (three seats per round, parallel); a Panel without isolated subagents is **excluded and
  reported, never scored as blind**.
- Write the run into `$E3/data/notes.md` (session ids, anything unusual, retries seen in `harness.retry` events, whether the confirmation was asked).

### Failures and stop rules

PREREG's, in short: an `agy` seat failure is retried once automatically (P4b) inside the run; if the run still ends with a harness failure, **re-run
it once** in a new session; a second failure **stands and is scored** (not excluded). A Panel red-team that ends in prose is **not** re-run.
**Quota or exhaustion (`harness_exhausted`, a usage limit from the host) ends the experiment INCOMPLETE on the spot**; do not wait and resume.
Never drop, re-run or replace a run because its result is disliked.

## 4. Tokens (counts, never dollars; two separate budgets)

- **Delphi-side (cap 2,000,000; 350,000 per run):** the daemon logs one `agent.usage` per span in
  `<home>/workspaces/delphi/local/missions/<mission>/events/`. For each B1/B2 run and each **A2** run, sum by the run's `task_id`, no `mode` filter:
  `python3 -I dev/train/e3/score_e3.py tokens <events dir> --task-id <task_id>`. Both attempts of a retried seat are in the sum.
- **Host-side (cap 400,000):** the Panel's seats are the host's own subagents; take the `result.modelUsage` total (input + output over all models,
  including subagents; cache excluded) from the stream of each Panel run. If the host reports nothing, write `NOT MEASURED` (never an estimate).
  For **A1** the panel home's events must show **0** Delphi-side tokens for the Panel's `task_id` (a check that nothing was spent daemon-side).
- Write `$E3/data/tokens.csv` with the columns **`question,run,side,tokens`** (`side` is `delphi` or `host`; an A2 run has one row of each),
  plus any notes as extra columns; then `python3 -I dev/train/e3/score_e3.py budget $E3/data/tokens.csv` prints the totals against the caps.
  **Run it after every question** (every run in phase 2): do not start a Council if `delphi_tokens + 135,130 > 2,000,000`, or a Panel if
  `host_tokens + 30,024 > 400,000`. If the planned spend passes 1,500,000 (the cap less the 500,000 reserve) before phase 2, drop the remaining
  B2 runs last question first. Planned: 1,279,200 Delphi-side, 234,000 host-side.

## 5. Score (reads only; once, after every planned run is done)

`$E3/manifest.json` (paths absolute or relative to the command's working directory; `null` or an absent key means not run; an object
`{"path": ..., "status": "capped|excluded", "why": ...}` marks a run the stop rules excluded):

```json
{"questions": [
  {"id": "q1", "b1": "records/q1-b1.md", "b2": "records/q1-b2.md", "a1": "records/q1-a1.md", "a2": "records/q1-a2.md"},
  {"id": "q2", "b1": "records/q2-b1.md", "b2": "records/q2-b2.md", "a1": "records/q2-a1.md", "a2": "records/q2-a2.md"}
]}
```

(The real file lists all six, q1 to q6, in that order, each with `b1`, `b2`, `a1` and `a2`.)

```sh
python3 -I dev/train/e3/score_e3.py analyze $E3/manifest.json > $E3/data/analysis.json
python3 -I dev/train/e3/score_e3.py baseline-e2 > $E3/data/baseline-e2.json
```

`analysis.json` carries the verdict for each arm (`SUPPORTED`, `NO EVIDENCE`, `WORSE`, `INCOMPLETE`; the headline is A1), every count the rule
used, the excluded runs and why, and the per-question classes. Run the analysis **once**; read it only after every planned run is graded.

## 6. Report and commit

`REPORT.md` in this directory, in the format of `dev/train/e2/REPORT.md`: the verdict line first (A1, then A2), the counts, the per-question
table (q1 and q2 shown separately: PREREG Amendment 1, threat 12) (class of each arm and B1/B2, escalated, readable seats, P0/P1 present), exclusions and why, the E2 baseline beside the E3 numbers, tokens
on both sides against the caps (and `NOT MEASURED` rows), the models and hosts as the records reported them, whether the confirmation was asked,
how many `harness.retry` events fired, the threats of PREREG (changed or not), and a statement that nothing about finding-level quality is claimed.
Copy `analysis.json`, `tokens.csv`, `notes.md` and the records into `dev/train/e3/data/` (they are Delphi Councils on this repository's own specs;
check nothing private is in them, and leave out the cards). A change to the section 4.5 notice is a separate pull request under PREREG's
"What each outcome changes". When done: `kill` the `caffeinate` pid, stop both daemons, `git worktree remove $E3/checkout`, and `rm -rf` the
directories you created (`$E3/home-council` and `$E3/home-panel` hold copies of the real configuration).
