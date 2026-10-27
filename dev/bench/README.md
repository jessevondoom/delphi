# Benchmark runner

The runner for [`evaluation-plan-2026-10-25.md`](../../missions/2026-10-23-foundational-design/evaluation-plan-2026-10-25.md). It is a small script (standard library only) that runs each arm on each task in a fresh clone, applies the hidden tests afterwards, and writes one JSON row per run.

```sh
python3 -I dev/bench/bench.py self-test                    # fake arms against a real git repository; run in CI
python3 -I dev/bench/bench.py run --dry-run  --tasks tasks/ --arms dev/bench/arms-example/ --out results.jsonl
python3 -I dev/bench/bench.py run --seeds 3 --tasks tasks/ --arms arms/ --out results.jsonl
```

**Nothing here spends money until you point it at real tasks and real arms.** The files in `arms-example/` show the shape; copy them, fix the paths, and read the cost caveats first.

## Rows
`task, arm, seed, started, exit_code, wall_secs, timed_out, cost_usd, arm_outcome, hidden_pass, hidden_tail` and, for a Delphi arm, `cards`, `ready_sha`, `ready_branch`, `merge_commit`, `decisions_awaiting_review`. `hidden_pass` is `true`, `false`, or `null` when the task has no hidden tests. A harness failure becomes a row with `error` set; the run goes on.

## Setting up a Delphi arm
A Delphi arm needs a **template home**: a `DELPHI_HOME` with one workspace (named `bench` by default) whose model access has been ratified once, by you, in the console (`delphi init`, add the endpoints, `/ratify`). For each run the runner copies that home, points `[workspace] projects` at the run's own clone (that table is not one of the pinned blocks, so the ratification still holds), and calls `delphi run … --json`. The template is never modified. Use API endpoints, not subscriptions, so that costs are metered for every arm (evaluation plan §2).

The run ends at "ready to merge" on a branch; the runner checks out that commit and grades it. `policy.toml` lists the cards a headless arm may allow (Amber only; Red and merges can never be allowed).

## A3b and T8: does the organization's bundle reach the work? (`specs/027` §7.2)
Arm **A3b** is a Delphi arm with `"bundle": "<dir>"` and `"paired_with": "<its A3 arm>"` (`arms-example/a3b-delphi-bundle.json`). Each run gets its own copy of the template home; the runner adds a **stripped copy** of the bundle to it (no `evals/`, so the hidden rubric cannot be read as `@bundle/…`; no signature; `delphi bundle manifest` is re-run) and counts the `bundle.cited` events the run wrote (`bundle_cited`).

Task set **T8** is a bundle's `evals/delphi/tasks/*.md`: frontmatter (`id`, `repo`, `base`, `set: pilot|confirmatory`, optional `timeout`), `## Prompt` (all an arm sees) and `## Rubric` (hidden; only its sha256 reaches the rows). The protocol, enforced by the runner:
```sh
# 1. pilot: the `set: pilot` tasks only (at least 8, disjoint from the confirmatory ones), 2 runs each, A3 and A3b
python3 -I dev/bench/bench.py run --pilot --tasks <bundle>/evals/delphi/tasks --arms arms/ --out results-pilot.jsonl
# 2. write the pre-registration (nine headings), commit it, pin it:  delphi bundle prereg check prereg-<bundle>-<date>.md
#    and record the printed blake3 in specs/027 §7.2. `## Task set` and `## Rubric` carry `delphi bundle prereg digest <tasks dir>` / `<rubric + grader prompt>`.
# 3. confirmatory: refused unless the file is whole, unchanged since the pin (only `## Deviations` may change) and names those digests
python3 -I dev/bench/bench.py run --tasks <bundle>/evals/delphi/tasks --arms arms/ --out results.jsonl \
    --prereg missions/<mission>/bench/prereg-<bundle>-<date>.md --prereg-pin blake3:<hex> --rubric <rubric dir>
```
Rows add `stage` (`pilot` or `confirmatory`), `task_set`, `bundle`, `bundle_hash`, `bundle_cited`, `rubric_sha256`, `graded` (always `null` here: the blind grader, from a model family not in the arm, fills it in later) and, for a confirmatory run, `prereg` and `prereg_pin`. A pilot's file name must contain `pilot`. blake3 comes from the `delphi` binary (`--delphi`), the same build that computes the pin.

Not built: the grader itself (rubric grading by a blind model family), McNemar and the bootstrap interval of the analysis plan, and a live run (no paid run has been made).

## Known gaps (also in the evaluation plan)
- **The spec step has no operator proxy yet.** `/build` against an issue text with no ratified spec works only for changes small enough for the bounded exemption (two files, 100 lines); a larger change stops at gate 3 ("No Spec, No Build"). That is a fair measurement of friction and an unfair one of capability. Deciding how to stand in for the operator here is an open decision.
- **The network is not restricted.** Run where egress is limited to the model endpoints.
- **The optimistic arm** (`--optimistic`) ends with exit code 5 and the branch kept; the runner grades the branch without reviewing the decisions, and records how many were waiting.
- Costs: a `command` arm reports cost only if `parse` can read it (`claude -p --output-format json` does). Otherwise `cost_usd` is `null` and you should not compare it.
