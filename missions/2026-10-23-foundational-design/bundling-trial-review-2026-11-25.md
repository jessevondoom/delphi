---
id: bundling-trial-review-2026-11-25
title: "Same-spec bundling trial: review for 2026-11-25 (data and recommendation)"
author: builder
timestamp: 2026-11-22T20:30:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [review, ci, bundling, merge-flow, measurement]
---

# Same-spec bundling trial: review for 2026-11-25

Data cut: 2026-11-22 20:10 UTC (main at `645c632a`, after #882). Window: pull requests merged from 2026-11-12 00:00 UTC to the cut. Prepared for the operator's decision on 2026-11-25 (CI velocity plan, slice K2). The rule under review is in [`CONTRIBUTING.md`](../../CONTRIBUTING.md) ("Trial, reviewed on 2026-11-25").

## Summary and recommendation

**Recommendation: continue, with changes.** One reversal trigger fired in its literal wording (a flake blocked one bundle twice), so the trial continues only if you decide that hit does not count. The data says it should not count: the flake also hits single-slice pull requests, and bundles needed fewer CI runs per slice than singles. The other two triggers did not fire.

- **Use.** 86 of the 220 merged pull requests (39%) carried 2 or more `Implements-Spec:` commits of one spec. 57 of them name 2 or more distinct slices. The rest are one slice plus review-fix commits that each carry the trailer.
- **Cost saved.** Bundled work needed **0.95 CI runs per slice**; single-slice pull requests needed **1.66**. That is about 127 to 235 fewer full runs, or about **41 to 79 macOS job-hours** saved in 11 days. The best single estimate is **about 43 hours**.
- **Quality.** No bundle was reverted, partly or fully. One bundle needed a follow-up fix within 48 h: #647, CI speed PR B, fixed by #659 (a job timeout). That change touched CI files only. Bundles were rerun less often than singles: 10% of bundles had a rerun, against 16% of singles.
- **Trigger 2 (flake blocks more than one slice twice): fired once.** In #792 (spec 128, slices S1 to S3), the known flaky `granted claude` version-probe test failed the macOS job twice on the same head. It passed on the third attempt. #735 (spec 114, M6 to M8) hit three different macOS flakes across two heads.
- **CI got faster, but not mainly because of bundling.** PR run p50 fell from 31.5 to 19.5 min and p95 from 68.6 to 26.8 min. macOS queue p95 fell from 39.4 to 2.5 min. Most of that comes from the CI speed work that merged the same day the trial started (#647, #649, #650, #654: one macOS job, split Linux tests, macOS skipped on drafts). Bundling only adds to it by cutting the number of runs.
- **The terms were often stretched.** Only 37 of 86 bundles stayed within both guidelines (3 slice commits or fewer, 800 changed lines or fewer). Two bundles mixed gate paths with code (#685, #819). One stack landing carried 8 specs at once (#724). The rules are not enforced, and the trial depends on discipline it does not get.

Proposed changes if you continue:

1. **Rewrite trigger 2** so it measures cost per slice: "reverse if bundles need more reruns per slice than singles over a review window". The current wording counts a flake that would have blocked a single-slice PR just as well. Or keep the wording and rule that #792 does not count. Either way, record the decision. A trigger that fires and is then ignored without a record weakens the rule.
2. **Make the size guideline a check.** The new `commits` job (#882) could warn when a PR has more than 3 slice commits or more than 800 changed lines. Review-fix commits should drop the `Implements-Spec:` trailer, or carry a `Fixes-Slice:` line, so slices can be counted.
3. **Keep gate paths out of bundles** (#685 mixed `.github/workflows/dev.yml` with `selfupdate.rs`). Treat stack landings like #724 as a separate, operator-only case, not a bundle.
4. **Merge every bundle with `delphi merge`.** 20 bundles have no ledger row at all because they were merged another way. A bundle the ledger does not hold cannot be attributed by the ledger. That gap is not caused by bundling, but it limits the evidence for trigger 3.
5. **Fix or quarantine the granted-claude version-probe flake.** It is the test behind the trigger-2 hit, and it is already in the flaky-test notes.
6. **Review again** once 10 or more bundles have run under the `commits` job. Only #882 has run it so far, so the answer to the Council's P0 (were the commits between the first and the head tested?) is built but not yet measured.

## 1. Merged pull requests, classified

A slice commit is a commit in the PR's range (first parent of the merge to second parent, merge commits excluded) whose message has a line starting `Implements-Spec:`. All 220 pull requests were merged with a merge commit; none were squashed.

| Class | PRs | Slice commits | Size (add+del) p50 / p95 | Open to merge (h) p50 / p95 | CI runs per PR (mean) | PRs with a rerun | Reruns | PRs with a failed run | Follow-up fix within 48 h |
| :-- | --: | --: | --: | --: | --: | --: | --: | --: | --: |
| Single-slice | 122 | 122 | 186 / 1,136 | 1.5 / 8.2 | 1.66 | 19 (16%) | 22 | 11 | 0 (the one text match, on #673, is a base merge that names it) |
| Bundled (one spec, 2+ slice commits) | 86 | 321 (about 213 named slices) | 776 / 3,096 | 1.9 / 8.7 | 2.35 | 9 (10%) | 11 | 17 | 1 (#647, fixed by #659) |
| Multi-spec | 1 (#724) | 50 | 24,398 | 3.2 | 4 | 0 | 0 | 1 | 0 |
| Other (no trailer) | 11 | 0 | 48 / 1,125 | 0.6 / 3.0 | 1.55 | 0 | 0 | 2 | 1 (#751 fixed red main) |

Per slice: bundles used 202 runs for about 213 named slices (**0.95 runs per slice**); singles used 203 runs for 122 slices (**1.66**).

**Bundles by number** (slice commits / named slices): #882 (4/4), #871 (2/1), #866 (2/2), #865 (2/2), #863 (2/2), #858 (3/3), #854 (3/1), #851 (3/2), #829 (3/1), #827 (2/2), #826 (3/1), #824 (2/1), #823 (2/1), #820 (4/2), #819 (4/4), #812 (6/3), #810 (2/1), #803 (3/1), #801 (2/2), #800 (5/2), #799 (2/1), #798 (2/2), #796 (2/2), #793 (2/1), #792 (5/3), #791 (3/3), #787 (2/2), #785 (2/2), #781 (4/4), #780 (2/2), #779 (3/1), #774 (3/3), #773 (3/3), #767 (4/3), #766 (2/2), #765 (6/5), #763 (2/1), #758 (3/3), #755 (2/1), #750 (2/2), #747 (19/6), #746 (4/2), #744 (6/5), #743 (3/3), #742 (10/6), #741 (4/4), #740 (2/1), #739 (6/5), #738 (2/2), #736 (4/2), #735 (5/4), #734 (4/3), #733 (6/6), #730 (3/2), #713 (5/3), #712 (5/4), #711 (3/2), #710 (3/1), #709 (5/4), #708 (5/1), #706 (3/1), #705 (5/4), #701 (18/9), #699 (3/1), #696 (4/3), #695 (6/6), #694 (2/2), #693 (11/1), #692 (2/2), #691 (5/5), #690 (3/3), #689 (5/5), #688 (3/3), #685 (2/1), #679 (2/1), #678 (4/3), #677 (2/1), #676 (4/1), #675 (2/1), #672 (2/1), #668 (2/2), #658 (2/1), #657 (3/3), #654 (2/1), #653 (2/1), #647 (3/1).

Distribution of slice commits per bundle: 2 (33), 3 (21), 4 (12), 5 (10), 6 (6), 10, 11, 18, 19 (one each).

Notes:
- #709 to #714 merged into stacked branches, not `main`. They reached `main` inside #724, which landed the knowledge-loop stack: specs 112 to 120, 49 commits. Strictly, #724 is a cross-spec merge to `main`. It was a stack landing at the operator's terminal, not a bundle opened under the trial, and it is reported here as a deviation from the terms.
- "Named slices" are distinct slice IDs (for example `S1`, `C3a`, `G7`, `§15`) taken from the subjects of the slice commits and from the PR title, and capped at the commit count. It is a heuristic. A bundle with no IDs found counts as 1.

### Compliance with the trial's terms

| Term | Result |
| :-- | :-- |
| Merge commit, not squash | 86 / 86 |
| One spec only | 86 / 86 same-spec; #724 (stack landing) is the one cross-spec merge |
| About 3 slices / 800 lines | 37 / 86 within both; 33 had more than 3 slice commits; 43 had more than 800 changed lines |
| No gate paths mixed with code | 2 exceptions: #685 (`.github/workflows/dev.yml` with `crates/delphi/src/selfupdate.rs`) and #819 (`GOVERNANCE.md` with pack-size test fixtures that the charter text changes). Gate-only bundles: #882, #799, #763, #647 |

## 2. Reversal triggers

### Trigger 1: a bundled PR needs a partial revert. **No.**

- No commit on `main` since 2026-11-11 is a revert: there is no `Revert "…"` subject and no `This reverts commit`. Three commit bodies use the word "revert", and all three describe something else: c58d03ab8 (a "denied or reverted" metric), c69f24337 (a change dropped before merge) and 6115ab1d8 (a roll-back table).
- Follow-up fixes within 48 h that name a bundle: #659 fixes the job timeout set in #647 (CI speed PR B, CI files only). It changed a timeout and removed nothing from #647. Other matches were base merges that only mention a PR number (bae6f4d06, 954496fae, e7a4a0716) or a plan update (36e263f23), not fixes.

### Trigger 2: a flake blocks more than one slice twice. **Yes, once, by the letter.**

| PR | Slices | What failed | Same head? | Result |
| :-- | :-- | :-- | :-- | :-- |
| #792 (spec 128) | S1-S3, 5 slice commits | macOS, attempts 1 and 2: `authorization_grant::tests::claude_seat::a_claude_builder_seat_with_only_bridge_tools_…` (the known granted-claude `--version` probe flake) | yes (`00b61484`) | green on attempt 3, no code change |
| #735 (spec 114) | M6-M8, 5 slice commits | macOS: `a_cli_that_lists_a_tool_of_its_own_in_a_granted_run_…`, then `kb::tests::episodes_are_indexed_…` on a rerun, then `a_granted_seat_whose_cli_loaded_any_server_…` on a later head | two heads | green after a rerun |
| #736, #741, #677, #668 | 2-4 each | macOS failed once, green on the rerun | yes | one block each |
| #746, #747 | spec 121 S0-S1, spec 118 P0-P5 | Linux unit `investigation_tests::…` twice | yes | not a flake: the ubuntu-26.04 runner change (git 2.56) that turned `main` red, fixed by #751 |
| #708 | 5 | `linux sandbox (bubblewrap)` once | yes | green on rerun |

How to read this: the #792 flake would have blocked a single-slice PR in the same way. The trigger was meant to catch bundling that makes flakes cost more. Measured per slice, it does not: bundles had 11 reruns for about 213 slices, singles had 22 reruns for 122. The trigger still fired as written, so continuing needs your explicit decision (change 1 above).

### Trigger 3: the merge ledger cannot attribute per-slice specs. **No, for every bundle it holds.**

`~/.delphi/merge/ledger.jsonl`, read only: 468 rows, of which 226 are `merged`.

| Ledger state for the 86 bundles (and #724) | Count |
| :-- | --: |
| `merged` row with a `specs` list that holds the bundle's spec, `head_sha` equal to the merged head | 49 |
| `merged` row with only the single `spec` field (before the multi-spec fix; first `specs` list at 2026-11-15 16:02 UTC); the spec matches, `head_sha` matches | 17 |
| No `merged` row (merged outside `delphi merge`) | 20 bundles + #724 |

- For a same-spec bundle the spec is the same for every slice, so the single-spec rows written before the fix attribute it correctly. The ledger caveat in `CONTRIBUTING.md` (only the first spec of a multi-trailer PR was recorded) mattered only for cross-spec PRs. The fix has been live since 2026-11-15.
- The ledger never records slice IDs, for singles or for bundles. It records the head commit, and the `Implements-Spec:` trailer of every slice commit can be recovered from it (`git log <base>..<head_sha>`). Every bundle row's `head_sha` matched the merged head, so that lookup works for all 66.
- Bundles merged outside `delphi merge` have no ledger attribution at all: #677, #685, #688 to #696, #699, #701, #705, #706, #746, #747, #750, #755 and #765, plus #724. Their attribution exists only in git. This is a gap in how they were merged, not in bundling.

## 3. CI timings

Run wall time is `updatedAt - createdAt` for successful first-attempt runs of the `ci` workflow (`gh run list`, the last 1,000 runs, which go back to 2026-11-07). "Before" covers 2026-11-07 to 2026-11-11, so the baseline is only five days.

| Runs | Before 2026-11-12 | From 2026-11-12 |
| :-- | :-- | :-- |
| pull_request, p50 / p95 (min) | 31.5 / 68.6 (n=162) | **19.5 / 26.8** (n=329) |
| push to main, p50 / p95 (min) | 30.4 / 53.0 (n=104) | 20.0 / 28.0 (n=170) |
| PR runs: total, failed, cancelled, rerun attempts | 218, 41, 4, 11 | 448, 44, 49, 30 |

macOS jobs, from a random sample of 70 first-attempt runs (30 before, 40 after). The jobs come from `gh api …/runs/<id>/jobs`. Queue time is the job's `started_at` minus the run's `created_at`. Before the change the macOS work was two jobs, `test (macos-latest)` and `lint (macos-latest)`. After it, it is one job, `macos (test + clippy)`, which does not run on draft PRs, so 33 of the 40 later runs had one.

| macOS job | Before (34 jobs, 30 runs) | After (33 jobs, 33 runs) |
| :-- | :-- | :-- |
| Queue p50 / p95 / max (min) | 0.1 / **39.4** / 58.0 | 0.1 / **2.5** / 5.1 |
| Duration p50 / p95 (min) | 26.1 / 33.2 | 20.2 / 24.3 |

The improvement came mostly from the CI speed work merged on 2026-11-11 and 2026-11-12 (#647, #649, #650, #654): one macOS job instead of two, macOS skipped on drafts, Linux tests split, and older runs cancelled on a new push (that is why cancelled runs rose from 4 to 49). Bundling helps keep the macOS queue short: the account may run only 5 macOS jobs at once, and fewer runs means less contention. This data cannot separate the two effects.

## 4. macOS minutes saved by bundling

The duration used for one macOS job is the later p50, 20.2 min.

| Estimate | Runs avoided | macOS time |
| :-- | --: | --: |
| Brief's formula, k = named slices (k-1 per bundle) | 127 | 2,565 min (about 43 h) |
| Brief's formula, k = slice commits (upper bound) | 235 | 4,747 min (about 79 h) |
| Observed rate: 213 slices × 1.66 runs/slice − 202 actual runs, × 0.8 share of runs with a macOS job | about 152 runs, about 122 with macOS | about 2,460 min (about 41 h) |

The best estimate is **about 40 to 45 macOS hours in 11 days**, about 4 hours a day. The estimate assumes every slice would otherwise have been a single PR with the single-PR rerun rate.

## 5. Caveats

- **Confounding.** The CI speed changes landed the same day the trial began, so the before and after timings mostly measure those changes, not bundling.
- **Slice counting.** The `Implements-Spec:` trailer is also put on review-fix commits, so "slice commits" overstates slices (321 commits against about 213 named slices). Named slices come from a regex over commit subjects and titles.
- **Run-to-PR matching.** Runs are matched to PRs by head branch name. A branch name reused across PRs would merge their counts. None of the 220 PRs lacked runs.
- **Follow-up detection.** It is text-based: a later PR title or commit naming `#N` with fix, follow-up, revert or after. A fix that does not name the PR would be missed.
- **Flakes.** Only the logs of bundles with a failed or rerun attempt were read. Singles were counted, not diagnosed.
- **Ledger.** The ledger is on this machine only. Merges made elsewhere or with `gh` leave no row.
- **The `commits` job (#882).** It has run on one bundle so far, #882 itself, so its effect is unmeasured.
- **Baseline.** The pre-trial baseline is five days long (the 1,000-run limit of `gh run list`).

## 6. Method

The scripts were run from a scratch directory outside the repository and are not committed.

```sh
# merged PRs in the window
gh pr list --state merged --search "merged:>=2026-11-12" --limit 400 \
  --json number,title,createdAt,mergedAt,additions,deletions,mergeCommit,headRefName,baseRefName
# slice commits per PR: for each merge commit M with parents P1 P2
git rev-list --parents -n1 M
git rev-list --no-merges P1..P2        # then git log -1 --format=%B <sha>, count lines starting "Implements-Spec:"
git diff --name-only P1...P2           # gate paths (.github/, GOVERNANCE.md, REVIEW.md) vs code (crates/, ui/src, ui/test)
# CI runs
gh run list --workflow ci.yml --limit 1000 \
  --json databaseId,headBranch,headSha,event,createdAt,updatedAt,startedAt,conclusion,attempt
gh api "repos/{owner}/{repo}/actions/runs/<id>/jobs?per_page=100"               # macOS queue and duration
gh api "repos/{owner}/{repo}/actions/runs/<id>/attempts/<n>/jobs?per_page=100"  # which job failed per attempt
gh run view <id> --attempt <n> --log-failed                                      # which test failed
# reverts and follow-ups
git log origin/main --since=2026-11-11 --format='%H%x1f%cI%x1f%B%x1e'           # grep revert / follow-up / fix #N
# ledger (read only)
~/.delphi/merge/ledger.jsonl                                                     # kind=merged rows: pr, head_sha, spec, specs
```

Percentiles use linear interpolation. The macOS sample was drawn with a fixed seed (1125).
