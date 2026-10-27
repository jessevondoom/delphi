# Headless runs and CI

`delphi run "<task>"` runs one task with no terminal, prints a summary and exits with a code that says what happened. It drives the same daemon through the same input as the console (`/build …`, `/council …`, `@role …` or a plain prompt) and adds no authority. The design is [`specs/016`](../../specs/016-headless-run.md).

```sh
delphi run "/build implement specs/011-retry.md in @project/my-project" \
  --on-card policy:ci-cards.toml --budget 3 --timeout 30m --json
```

## Cards

A run has nobody to click `/y`, so every approval card is answered by `--on-card`:

| Value | What happens |
| :-- | :-- |
| `deny` (default) | every card is denied, with the reason recorded |
| `policy:<file>` | cards that match an `[[allow]]` rule are allowed; everything else is denied |
| `relay` | nothing is answered; cards wait for you through [`delphi relay`](away-mode.md) or an attached console, bounded by `--timeout` |

A policy file lists Amber cards only:

```toml
[[allow]]
tool = "write_file"

[[allow]]
tool = "run_command"
pattern = "cargo test*"      # optional; * matches anything
```

Three rules hold under any policy, and a policy that breaks them does not load: **Red cards are never allowed**, **merges are never allowed** (a policy naming `merge` or `pull_request` is refused), and unknown keys are errors. A build that reaches the merge card is the normal end: the run records the branch, commit and gate report, denies the card and reports `ready_to_merge`. You merge later from the console.

## Output

| Flag | Stdout |
| :-- | :-- |
| (none) | a short human summary |
| `--json` | only the summary object |
| `--jsonl` | every event of the mission as it happens, then the summary as the last line |

The summary has `"type": "run.summary"` and carries the outcome, exit code, mission and task IDs, `ready_to_merge` (`sha`, `branch`, `files`), the merge commit or pull request URL, the last gate report, the escalation reason, the first 4,000 characters of the final reply, every card seen and what was done with it, usage totals from the ledger, and wall time. Event lines follow [`schemas/event-envelope.schema.json`](../../schemas/event-envelope.schema.json).

## Exit codes

| Exit | Meaning |
| :-- | :-- |
| 0 | completed, ready to merge, merged, or pull request opened |
| 1 | gates failed, the retry ladder was used up, or an agent halted |
| 2 | a non-merge card was denied and the task escalated |
| 3 | over budget |
| 4 | failed, incomplete, timed out, or the run could not start (bad arguments, no daemon, connection lost) |
| 5 | an optimistic build finished and its provisional decisions wait for you (`/decisions`, then `/merge <task>`) |

A budget stop outranks a denied card, and an outright failure outranks everything.

## Other options

| Option | Effect |
| :-- | :-- |
| `--permissions <mode>` | the daemon's approval mode for this run; the previous mode is restored afterwards |
| `--budget <usd>` | the mission ceiling for this run |
| `--timeout <dur>` | cancel the task and exit 4 after this long (`30m`, `2h`) |
| `--mission <id>` | continue an existing mission |
| `--keep-daemon` | leave the daemon running afterwards |
| `--workspace <name>`, `--home <dir>` | choose the workspace and home |

## In CI

- Give the job its own home: `--home "$RUNNER_TEMP/delphi"` (or `DELPHI_HOME`), and set up model access there. Credentials come from the environment variables your endpoints name, never from `config.toml`.
- If a daemon is already running for that workspace and the process has no terminal, the run stops with exit 4 and says how to proceed. Use a fresh home, or `delphi daemon stop`.
- Treat exit 0 with `ready_to_merge` as "passed my gates, awaiting a human", which is what it is.
- This repository's own CI uses the same machinery for its benchmark runner: see [`dev/bench/README.md`](../../dev/bench/README.md).
