# Training mode

Cases that test Delphi's own behavior, for the maintainers (`specs/031` §4). `delphi train validate` checks every case in
`suites/`; `delphi train plan --budget-usd N` lists the cases a run would use and the most it could cost. Neither runs
anything. Running the cases, baseline against candidate, is the next slice (L6b).

```sh
delphi train validate
delphi train plan --suite orchestration --budget-usd 5
```

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

The two seed cases were written from the specs and have not been run on a live daemon. Treat their checks as hypotheses
until a run confirms them. Keep holdout cases out of anything an author seat can read.
