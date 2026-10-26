# Training mode

Cases that test Delphi's own behavior, for the maintainers (`specs/031` §4). `delphi train validate` checks every case in
`suites/`; `delphi train plan --budget-usd N` lists the cases a run would use and the most it could cost. Neither runs
anything. `delphi train run` runs them, baseline against an optional candidate overlay, and **spends money**.

```sh
delphi train validate
delphi train plan --suite orchestration --budget-usd 5
# a template home is a copy of a home whose model access you ratified once; never your real home
delphi train run --suite orchestration --budget-usd 5 --template-home ~/delphi-train-template --work-dir /tmp/train-1 --seeds 3
# candidate: a directory of changed ratified files, laid out as the workspace's logical paths
delphi train run --budget-usd 10 --template-home ~/delphi-train-template --work-dir /tmp/train-2 --overlay ./candidate --seeds 3
```

An overlay directory holds protected files at their logical paths (`agents/builder/AGENT.md`, `context/heuristics.md`, ...).
The runner writes them, with their hashes, into the temp copy of the home only. Each run lives under the work directory
(`001-baseline-<case>-s1/` with its `home/`, `repo/`, `stdout.txt`); `rows.jsonl` has one row per run. The report says
"no evidence" unless there are at least three seeds per arm and the arms separate completely.

## A case

`suites/<suite>/cases/<id>/case.toml` with a `fixture/` directory beside it for task and probe cases. A case names its
`role`, `split` (`dev` or `holdout`), failure-mode `tags` (`FM-1.2` from `context/domains/mas-architecture-and-failure-modes.md`),
a cost cap, a time cap and at least one deterministic `[[check]]`:

| check | passes when |
| :-- | :-- |
| `outcome` (`is`) | the task ended `done`, `escalated`, `failed` or `cancelled` |
| `event_count` (`event`, `eq` or `at_least`/`at_most`) | the run's log holds that many events of the type |
| `event_absent` (`event`) | the log holds none |
| `file_unchanged`, `file_exists` (`path`) | the fixture file is byte-identical, or exists, after the run |
| `file_contains` (`path`, `text`) | the fixture file contains the text |

## Status

The two seed cases were written from the specs and have not been run on a live daemon (the runner itself is tested with a
fake `delphi` script only). Treat their checks as hypotheses
until a run confirms them. Keep holdout cases out of anything an author seat can read.
