#!/usr/bin/env bash
# Spec 125-A3 (section 12): rerun a `ci` run once when the only reason it failed is a macOS job that never got a runner.
#
# GitHub cancels a hosted job it could not hand to a runner after about 15 minutes ("The job was not acquired by Runner
# of type hosted even after multiple attempts", beside "Due to capacity constraints, jobs targeting macOS arm64 runners
# may experience longer queue times"). Such a job ends `cancelled` with no steps and no runner, and `ci-gate` fails.
#
# Usage: macos_rerun.sh <owner/repo> <run id> <run attempt> <event> <head branch>
# Reruns the failed jobs (`rerun-failed-jobs`) only when all of these hold; otherwise it prints why not and exits 0:
#   1. this is the run's first attempt (so a run is rerun at most once by this script);
#   2. at least one job whose name starts with "macos" ended `cancelled` with no steps and no runner, and carries
#      GitHub's "not acquired" annotation (a job someone cancelled by hand does not);
#   3. every other job succeeded or was skipped, apart from `ci-gate` (which fails because of 2): a real failure is
#      left for a person to read, never rerun blindly;
#   4. this run is still the newest `ci` run for its branch and event: rerunning an older pull request run would join
#      that pull request's concurrency group and cancel the newer run.
# DRY_RUN=1 prints the call instead of making it.
set -eo pipefail

repo="$1" run="$2" attempt="$3" event="$4" branch="$5"
if [ -z "$repo" ] || [ -z "$run" ] || [ -z "$attempt" ] || [ -z "$event" ] || [ -z "$branch" ]; then
  echo "usage: macos_rerun.sh <owner/repo> <run id> <run attempt> <event> <head branch>"
  exit 2
fi

if [ "$attempt" != 1 ]; then
  echo "attempt $attempt of run $run: not the first attempt, no rerun"
  exit 0
fi

jobs="$(gh api "repos/$repo/actions/runs/$run/attempts/1/jobs?per_page=100")"
echo "$jobs" | jq -r '.jobs[] | "\(.name): \(.conclusion) (\(.steps | length) steps, runner \(.runner_name // "none"))"'

starved_ids="$(echo "$jobs" | jq -r '.jobs[]
  | select((.name | startswith("macos")) and .conclusion == "cancelled" and (.steps | length) == 0
           and ((.runner_name // "") == ""))
  | .id')"
if [ -z "$starved_ids" ]; then
  echo "no macOS job ended cancelled without a runner: no rerun"
  exit 0
fi

for id in $starved_ids; do
  notes="$(gh api "repos/$repo/check-runs/$id/annotations" --jq '.[].message')"
  if ! grep -q "was not acquired by Runner" <<<"$notes"; then
    echo "job $id ended cancelled with no steps but without GitHub's \"not acquired\" note: no rerun"
    exit 0
  fi
done

others="$(echo "$jobs" | jq -r --arg ids "$starved_ids" '($ids | split("\n")) as $s | .jobs[]
  | select(.name != "ci-gate" and .conclusion != "success" and .conclusion != "skipped"
           and ((.id | tostring) as $i | $s | index($i) | not))
  | "\(.name) (\(.conclusion))"')"
if [ -n "$others" ]; then
  echo "other jobs did not pass, so this is left for a person:"
  echo "$others"
  exit 0
fi

enc_branch="$(jq -rn --arg b "$branch" '$b | @uri')"
newest="$(gh api "repos/$repo/actions/workflows/ci.yml/runs?branch=$enc_branch&event=$event&per_page=1" --jq '.workflow_runs[0].id')"
if [ "$newest" != "$run" ]; then
  echo "a newer ci run ($newest) exists for $branch ($event): no rerun"
  exit 0
fi

echo "only macOS jobs that never got a runner failed: rerunning the failed jobs of run $run once"
if [ "${DRY_RUN:-}" = 1 ]; then
  echo "DRY_RUN: gh api -X POST repos/$repo/actions/runs/$run/rerun-failed-jobs"
else
  gh api -X POST "repos/$repo/actions/runs/$run/rerun-failed-jobs"
fi
