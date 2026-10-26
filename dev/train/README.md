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

## Arms (`specs/037` §2)

`delphi train run --arms-file arms.toml` runs every arm on every case and seed; each arm has its own template home (its
routing is configuration) and may declare `expect_models`, the model ids or prefixes it may use. A run in which any
other model made a call is not graded: it no longer measures that arm. See `arms-pilot-example.toml`. Rows record tokens
in and out, calls and cost per model, attempts and mid-run model swaps.

## Reviewing a truth pack (your job before any research run)

Each research case has a generated **review sheet** beside its pack: open `truth/review-sheet.md` in
`suites/research/cases/<id>/truth/` (`motogp-2027-regs`, `mycorrhizal-networks`, `phantom-paper`). It puts every fact beside
its source link, the verbatim quote and the patterns, lists the traps, sources and rubric, and shows the judgement questions
with room for your own view. Regenerate after any change with `delphi train truth-sheet --all`.

```sh
delphi train truth-sheet --case research/phantom-paper          # print the sheet
delphi train truth-check --case research/phantom-paper --text-file answer.md   # what the checks make of an answer you wrote
delphi train verify --case research/phantom-paper --by "Your Name"            # when you are satisfied
```

`verify` records your name, the date and a **fingerprint** of everything you checked (facts, traps, sources and extracts,
rubric, insights). Any later edit makes the stamp stale and the pack a draft again.

**Facts and judgement are different things to review.** Facts are checked against their sources (the pack already confirmed each
quote appears in its extract; your part is whether the claim means what the quote says and the source is good enough).
**Judgement calls** (`truth/insights.toml`) are strategic questions or insights where reasonable people differ: the assistant
wrote a starting view (never shown to any judge), and **only the `operator_view` you write reaches a judge, as a view, not a
fact**. That is how the study tests the jury on judgement and not only on recall: reward reasoning toward your view or a
well-argued alternative, penalize unsupported certainty.

## Which models can I use right now?

```sh
python3 -I dev/train/avail.py --workspace <name>                 # no model is called: ~2 s
python3 -I dev/train/avail.py --workspace <name> --live --json avail.json   # one tiny call per model, in parallel: ~40 s
```

It checks three layers, cheapest first, and only calls a model where nothing cheaper can answer. **Offline**: the workspace's
tiers and the saved pool state (a model marked exhausted until a time). **Deterministic**: `claude auth status`, `agy models`
(the account's catalogue), Ollama's `/api/tags`, and the model-list endpoint of any API provider whose key is in the
environment; these say whether a model is installed and offered, never whether quota is left, because a listing cannot see a
quota. **Live** (`--live`): the Claude CLI reports its five-hour and seven-day utilization as part of the call, and agy says
`quota reached ... Resets in 101h`. A model with no cheaper signal is `unknown` until `--live`. Read-only on the Delphi home;
no credential is printed. The JSON feeds the arm template homes.

Subscription seats have no list endpoint to ask. If a deterministic quota endpoint for a subscription is wanted (it would
avoid even the one-token calls), it would read the CLI's OAuth token from the keychain, so it is not built unprompted.

## Building the arm homes

```sh
python3 -I dev/train/avail.py --workspace <name> --live --json avail.json
delphi train arms init --source-home ~/.delphi --workspace <name> --defs dev/train/arms-defs.toml \
    --avail avail.json --out /path/to/new/dir
delphi train run --arms-file /path/to/new/dir/arms.toml --work-dir /path/to/run --no-session-cap --allow-unverified ...
```

Each arm is a throwaway copy of your workspace's configuration and ratified files, with the tier models, `[access]` and
`role_family` rewritten as `arms-defs.toml` says and the pinned hash of those blocks re-sealed (as `/ratify` would). It carries
no missions, state, worktrees, or your own lessons, heuristics and profile, so two arms differ only by their staffing. Your
home is only read. Each built home is loaded the way the daemon loads it and the models its pool would use are printed, so a
typo or an unreachable endpoint shows up here. An arm whose model `avail.json` says is exhausted or unlisted is skipped with
the reset time. `expect_models` in the generated `arms.toml` holds each model's alias and its `claude-` long form (the Claude
CLI may report either; the first live run confirms which).

## Spending: caps

`train run` and `train judge` take `--budget-usd N`, or **`--no-session-cap`** to drop the session total on purpose. The cap on
each case (`max_cost_usd`, passed to `delphi run --budget`) and on each judge call stays in both modes: it is what stops one
runaway run. The runner also kills a run at its timeout.

## Judging a run (`specs/037` §6)

A `delphi train run` leaves a bundle per graded run in `<work>/bundles/`, filed under a blind id with the arm and model
names scrubbed; the key is `bundles/key.jsonl`. Then:

```sh
delphi train judge --work-dir W --judges-file judges.toml --budget-usd 20     # a jury of other models, both orders per pair
delphi train review --work-dir W                                               # you, blind, before seeing the jury
delphi train disputes --work-dir W                                             # where you and the jury differ
delphi train resolve --work-dir W --id d1234abcd --cause rubric_gap --note "..."
delphi train report --work-dir W --judges-file judges.toml
```

`judges.toml` is an arms file (`[[arm]]` per judge: a template home and the `expect_models` it uses). A judge never scores a
pair that includes its own model. A pair counts for a judge only if both orders picked the same bundle. A disagreement is
never averaged: it is contested until you and the jury's reasons have been discussed and a cause recorded. The report says
"no evidence for a ranking" with fewer than three graded seeds per arm.

## Code cases from history (`specs/037` §3, §5.2)

A code case starts the agent from an earlier commit and scores it on the tests of the change that followed. It has
`spec = "spec.md"` (the task, ratified in the run's home; it must give the names the tests use) and a `hidden/` directory
that no run can read. Make one with:

```sh
delphi train harvest --case code/<id> --base <parent-sha> --reference <change-sha> \
    --test-file crates/foo/src/bar.rs --cmd "cargo test -p foo --lib --locked -- bar::" --min-passed 10
```

Harvest extracts the reference's test module from each `--test-file`, then proves the case: the hidden tests must fail on
the base commit and pass three times in a row on the reference. `delphi train check-hidden --case code/<id>` repeats the
proof; any edit to the case invalidates it, and `delphi train run` refuses an unproven case without `--allow-unvalidated`.
It builds the project but calls no model. A run's project is the base tree with no history; after the run the test
module in each named file is replaced by the reference's and the command is run. `--policy <file>` is the Amber-card
policy for headless builds.

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
