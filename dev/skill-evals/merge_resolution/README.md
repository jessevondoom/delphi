# Merge-resolution evals

Behavior evals for the builder's `git-merge-and-conflict-resolution` skill (spec 094, AC-7b). They test whether a
resolution is right, not whether the skill is found (that is AC-7a, `dev/skill-evals/`).

Six seeded cases, each a fresh temp git repo (branch `feature` diverged from `main`, no remote): `tracker`
(double append to a markdown file), `modlist` (two `mod` lines), `generated` (`out.md` made by `./gen.sh`), `lockfile`
(the right answer is main's), `behind` (no conflict), `semantic` (merges cleanly, the gate `check.sh` then fails).

- `cases.py` builds the repos with plain git and a fixed identity.
- `checks.py` has one pure `check(repo) -> (ok, reason)` per case, run after the resolution is committed. All cases
  also require: no remote, clean tree, no markers, main untouched, base merged in by a merge commit, original branch
  commits still ancestors (no rebase).
- `run.py` has the self-test and the live runner. Python 3, standard library only.

## Self-test (free, offline, a few seconds)

    python3 -I dev/skill-evals/merge_resolution/run.py selftest

Every case runs a known-good canned resolution (must pass) and at least one known-bad one (keep one side, hand-merge,
hand-edited lockfile, rebase, committed with the gate red; must fail). Run it before trusting any live result. It exits
non-zero on a miss.

## Live mode (SPENDS TOKENS)

    python3 -I dev/skill-evals/merge_resolution/run.py live --dry-run
    python3 -I dev/skill-evals/merge_resolution/run.py live --model <label> --template-home <home> \
        --budget-tokens 400000 --yes [--case semantic]

It builds each repo, points a copy of a pre-ratified Delphi home at it, runs `delphi run "/build ..."` as
`dev/bench/bench.py` does (policy: `dev/bench/arms-example/policy.toml`), then applies the check. It refuses to run
without `--budget-tokens` and `--yes`, and stops starting new cases once the counted tokens reach the budget.
`--model` is a label for the report: the model comes from the template home's workspace config. `--dry-run` only
builds the repos and prints what it would run.

Not verified: the live path has never been run. Whether the builder can run `git merge` under that policy, and the
token count read from the run summary, are untested.
