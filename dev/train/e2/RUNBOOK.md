# E2 runbook (operator-run)

**Nothing in this file runs until the operator says go AND spec 086 S2 and S3 are merged.** Writing this file, `score.py` and the
tests spent no model request, and so does the dry run below. Every step from "Run" on calls a model and spends the operator's
tokens. The design, measures, decision rule, caps and stop rules are in `PREREG.md` and are not repeated or changed here.

Notation: `E2=/tmp/e2` (work directory, outside the repository), `SRC=~/.delphi/workspaces/delphi/local` (read only). Scratch
runs always use `--home`; a bare `delphi` would touch the real workspace.

## 0. Gate before anything runs

1. The operator's explicit go, in writing, with the host list (Claude Code, plus at most one other host whose S0b probe shows
   *parallel isolated subagents* `verified`) and the caps (PREREG "Budget"; the operator may lower them in a commit first).
2. `git log` shows S2 and S3 merged into the branch that is built, and S0a (the F-1 fix, commit 914a141 or its successor) is in it.
3. `PREREG.md`, `score.py` and `test_score.py` are committed and unchanged since registration (`git status` clean for
   `dev/train/e2/`).

## 1. Dry run (spends nothing; do it first, stop on any failure)

```sh
python3 -I dev/train/e2/test_score.py
mkdir -p $E2/q $E2/records
```

Checklist (each item is a read or a local script; none starts a model):

- [ ] `python3 -I dev/train/e2/test_score.py` passes (it also checks that `score.py` carries PREREG's constants).
- [ ] **Source records unchanged.** For each of the eight records in PREREG's table run `shasum -a 256 "$SRC/<relative path>"` and
  compare with the table. A mismatch is reported, not fixed: the question set is the table, and if a record changed, the
  question text is still extracted from the changed file only if the operator writes down that decision before the first run.
- [ ] **Extract the eight questions** (`$E2/q/q1.txt` ... `q8.txt`, in PREREG id order):
  `python3 -I dev/train/e2/score.py extract-question "$SRC/<relative path>" > $E2/q/q1.txt`. Read each file once. Record
  `shasum -a 256 $E2/q/*.txt` in the manifest notes. These bytes go to every arm unchanged.
- [ ] **Pointers exist.** `grep -E '^- [^ ]+:' $E2/q/q*.txt` lists each question's pointer paths; `ls` each in the checkout. Write
  the missing ones into the notes (PREREG: the run proceeds).
- [ ] **Score a record against itself and against another** (reads only):
  `python3 -I dev/train/e2/score.py score "$SRC/<rec a>" "$SRC/<rec b>"` returns JSON; against itself it must give
  `same_decision: true` and `overlap: 1.0` (or `both_empty`). This also shows the old records' `unreadable` damage.
- [ ] **Source cost for the caps.** Find each source Council's `task_id` in its mission's `events/*.jsonl` (the `council.verdict`
  event's `task_id`) and run
  `python3 -I dev/train/e2/score.py tokens "$SRC/missions/<mission>/events" --task-id <t_NNNN>`. Compare the totals with PREREG's
  caps (400,000 per run, 8,000,000 total). If a Council cost more than the cap, the operator lowers or keeps the caps in a commit
  before the first run; the caps are never raised after.
- [ ] **Scratch home with the source records and their sessions removed** (Isolation, below), then
  `grep -rIl -F "<first 60 characters of the question>" $E2/home` for each question: zero hits expected; hits are written into the
  notes (PREREG threat 6).
- [ ] **Skill text requires isolation.** Open the generated `delphi-council` skill for the host and confirm it tells the host to
  spawn one isolated seat per role and never to paste a peer's stance into another seat (a reading of the file, not a run).
- [ ] **Manifest skeleton** (`$E2/manifest.json`, see section 4) has all 8 ids, the host list and the headline host, and every path
  field is empty, not guessed.
- [ ] Disk: `df -h /tmp` shows room for two copies of the scratch home.

## 2. Isolation

Make one scratch home per arm family so a Panel's staged record cannot reach a daemon Council's memory, and the reverse:

```sh
cp -Rc ~/.delphi $E2/home-council     # cp -Rc: clone, not symlink (never symlink a shared directory)
cp -Rc ~/.delphi $E2/home-panel
```

In each copy delete the eight source records and their session folders under
`workspaces/delphi/local/` (the `councils/` files in the table, and the `sessions/host-*` directories and the two session
directories they sit in), and the previous Councils' lesson proposals if they are stored by those mission ids. Record what was
deleted in `$E2/notes.md`. Start the daemon for an arm with `--home $E2/home-council` (resp. `home-panel`). After every run,
the staged Panel record and the Council record stay in that arm's home and are **not** copied into the other.

## 3. Run (spends tokens; only after section 0)

Order, per question in id order: **B1, then A, then B2**, each in a **new session** (new `delphi mcp` connection, new host
session). The question file is pasted unchanged, with its pointers, as the first and only user message after the fixed prefix
below; nothing from earlier runs is available to the session.

**Arm B (daemon Council), runs B1 and B2.** In a fresh host session connected to the council home, send exactly:
`Ask Delphi's Council (request_council, the daemon Council, not a Panel) to rule on the following question, and print the
council_result display unparaphrased.` followed by the contents of `q<k>.txt`. Poll `council_status` until done.
Copy the record the daemon wrote (its `path` is in `council_result`) to `$E2/records/q<k>-b1.md` (resp. `-b2.md`). Note the
`council_id` and the daemon's `task_id` for the token step.

**Arm A (Panel).** In a fresh host session connected to the panel home, send exactly:
`Run a Panel with Delphi's delphi-council skill, with isolated subagent seats (blind), on the following question, and print the
council_result display unparaphrased.` followed by the contents of `q<k>.txt`. Answer the cost confirmation `council_frame`
asks for with "yes" (and nothing else; no steering). Copy the resulting record to `$E2/records/q<k>-panel-<host>.md` and the
printed ruling card to `$E2/records/q<k>-panel-<host>.card.md` (the card shows the `blind:` and `isolation:` lines). If the
record has no machine-readable `blind:` line, add a front-matter line `blind: yes` or `blind: no` to the **copy**, copied from the
card exactly, and note that you did; do not add anything else to a record.

A Panel that escalates is **not** followed by a daemon Council, and an escalated daemon Council is **not** answered with
`/follow` or `council_choose`; the record as written is the result.

**Check each record immediately** (reads only):
`python3 -I dev/train/e2/score.py question-check $E2/records/q<k>-<arm>.md $E2/q/q<k>.txt` must say
`contains_question_verbatim: true`. If false the moderator reworded the question: that run is excluded and re-run once.

**Stop rules** are PREREG's: re-run a failed run once; stop when a cap is reached; a host with no isolated seats is excluded and
reported. Do not skip, reorder or replace a question.

## 4. Tokens (counts, never dollars)

- **Daemon arm (B1, B2):** the daemon logs one `agent.usage` event per span in `<home>/workspaces/delphi/local/missions/<mission>/events/`
  (`<mission_dir>/events/NNNNNN.jsonl`). Sum them by the Council's `task_id`, no `mode` filter (spec 078 section 1):
  `python3 -I dev/train/e2/score.py tokens <home>/workspaces/delphi/local/missions/<mission>/events --task-id <task_id>`
  prints `tokens` (input + output + thinking), `cache_read`, `cache_write`, `events` and the `models` that made a call. Write it
  into `$E2/tokens.csv` as `question,run,tokens,cache_read,cache_write,events,models`.
- **Panel arm (A):** the seats are the host's own subagents, so their tokens are the host's. Write the total the host reports
  (the host's usage line or the ruling card's cost line) as `tokens`, with the host's own words about what it counted in a
  `note` column. If the host reports nothing, write `NOT MEASURED` (never an estimate). The daemon side of a Panel (briefs,
  submits) makes no model call; the `tokens` command run on the panel home's events must show `events: 0` for the Panel's
  `task_id`, which is a check that nothing was spent daemon-side.
- Total after each question; stop per PREREG when a cap would be passed.

## 5. Score (reads only; after every planned run is done)

Write `$E2/manifest.json`:

```json
{"headline": "claude-code",
 "questions": [{"id": "q1", "b1": "records/q1-b1.md", "b2": "records/q1-b2.md",
                "panel": {"claude-code": "records/q1-panel-claude-code.md"}}]}
```

(paths absolute or relative to the working directory of the command; one entry per id `q1`..`q8`; extra hosts get extra keys
under `panel` and are scored by a second manifest whose `headline` is that host.)

```sh
python3 -I dev/train/e2/score.py analyze $E2/manifest.json > $E2/analysis.json
python3 -I dev/train/e2/score.py score $E2/records/q1-panel-claude-code.md $E2/records/q1-b1.md --panel-a   # one pair, for the report
```

`analysis.json` carries the verdict (`SUPPORTED`, `NO EVIDENCE`, `WORSE`, `INCOMPLETE`), the counts the rule used, the excluded
questions and why, and `venue_checks` per record. Run the analysis **once**. The experimenter reads the table only after every
planned run is graded.

## 6. Report and commit

`REPORT.md` in this directory, in the format of `dev/train/e1/REPORT.md`: the verdict line first, then the counts, the per-question table,
exclusions and why, tokens by arm and total against the caps (and `NOT MEASURED` rows), the models and hosts as the records
reported them, the venue cross-checks, the threats of PREREG (changed or not), and a few matched and unmatched finding pairs
verbatim. Copy `analysis.json`, `tokens.csv`, `notes.md` and the records into `dev/train/e2/data/` (the records are Delphi
Councils on this repository's own specs; check nothing private is in them before committing). Any change to the section 4.5
notice is a separate pull request under PREREG's "What each outcome changes". Remove `$E2` scratch homes when done
(`rm -rf` the directories you created; they hold copies of the real workspace).
