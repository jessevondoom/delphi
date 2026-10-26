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

## Research cases and truth packs (`specs/037`)

A `kind = "research"` case has no `fixture/`; it has a `truth/` directory beside `case.toml` that is **never copied into a
run**: `meta.toml` (as-of date; `verified_by` and `verified_on`), `facts.toml`, `traps.toml`, `sources.toml` with extracts in
`sources/`, and `rubric.md`. `delphi train validate` checks a pack against itself (every fact names a source, every quote is
in that source's extract, every extract matches its recorded hash, every pattern compiles). After writing or editing an
extract run `delphi train seal --case <suite>/<id>` to record its hash.

| check | passes when |
| :-- | :-- |
| `fact_recall` (`min`) | the deliverable states at least that weighted share of `facts.toml` |
| `no_traps` | no pattern in `traps.toml` matches |
| `text_matches` (`pattern`, `present`) | the pattern does (or, with `present = false`, does not) appear |

The deliverable is the task's final answer plus any Markdown the mission wrote. Patterns give a **lower bound** (a correct
answer worded unexpectedly is missed), so thresholds are modest and a jury reads the rest against `rubric.md`. A pack that
no operator has verified is a draft: it is listed by `validate` and `delphi train run` refuses it without `--allow-unverified`.
Verifying means reading each fact against its source, then setting `verified_by` and `verified_on`.

## Status

The two seed cases were written from the specs and have not been run on a live daemon (the runner itself is tested with a
fake `delphi` script only). Treat their checks as hypotheses
until a run confirms them. Keep holdout cases out of anything an author seat can read.
