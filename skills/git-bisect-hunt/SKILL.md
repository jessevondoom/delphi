---
name: git-bisect-hunt
description: "Find the commit that introduced a regression with an automated reproduction and git bisect in a detached checkout. Use when something used to work."
authorized_agents:
  - investigator
input_schema:
  type: object
  required: ["repro"]
  properties:
    repro:
      type: string
      description: "A command that exits non-zero when the bug is present"
    good:
      type: string
      description: "Known-good revision"
    bad:
      type: string
      description: "Known-bad revision"
output_schema:
  type: object
  required: ["culprit"]
  properties:
    culprit:
      type: string
      description: "The first bad commit with the evidence"
required_permissions:
  - read:all
  - shell:scoped_scripts
---
# Git Bisect Hunt

Bisect turns "it broke sometime" into a single commit. It is only as good as the reproduction.

## When to use
- A behaviour regressed and a known-good revision exists.

## Steps
1. Make the reproduction a single command that exits 0 when good and non-zero when bad, and that fails for the reason in the bug report, not for build or setup errors.
2. Confirm it on both ends: it must pass at the good revision and fail at the bad one. If not, fix the reproduction first.
3. Work in a separate detached worktree or clone so the operator's checkout is never moved.
4. Run `git bisect run` with the reproduction. Treat commits that do not build as skipped, not bad.
5. Read the culprit commit and confirm the mechanism: revert it locally in the worktree and check that the bug disappears.
6. Report the commit, the reproduction command, and the mechanism. Reset the bisect and remove the worktree.

## Done when
The culprit is identified, the mechanism explained, and the revert experiment confirms it.

## Pitfalls
- A flaky reproduction that sends bisect the wrong way: run it several times at each end.
- Reporting the commit without saying why it breaks.
