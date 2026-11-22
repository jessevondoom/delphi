---
name: git-pr-packaging
description: "Prepare atomic commits and a pull request with spec linkage, a checked diff, and honest verification notes. Use when work is ready for review."
authorized_agents:
  - builder
input_schema:
  type: object
  required: ["branch"]
  properties:
    branch:
      type: string
      description: "Task branch to package"
    spec:
      type: string
      description: "Spec the work implements"
output_schema:
  type: object
  required: ["pr"]
  properties:
    pr:
      type: string
      description: "Prepared commit series and PR description"
required_permissions:
  - read:all
  - write:worktree
  - shell:scoped_scripts
---
# Git and PR Packaging

A reviewer should be able to understand the change from the commits and the description alone.

## When to use
- The task's tests are green and the work is ready for review.
- Pushing, opening a PR and merging are outward-facing: do them only when the task or the operator authorizes it.

## Steps
1. Read your own diff first. Remove debug output, stray files, commented-out code, and anything unrelated.
2. Search the diff for secrets, tokens and absolute home paths. Remove them.
3. Split into atomic commits: each builds and passes tests on its own, with a message saying why.
4. Add the trailer `Implements-Spec: specs/NNN-name.md` where a spec applies.
5. Write the description: what changed, why, how it was verified (commands and results, including anything skipped), and risks. Where a spec applies, add (`specs/133` §2.1.1):
   - a table `| Criterion | Test or check | Result |`, one row per criterion the slice closes; a result is `pass`, `fail`, `not run`, `not run live` or `not verified`;
   - `Deviations:` `none`, or each place the build departed from the spec text (`GOVERNANCE.md` §1.4);
   - `Follow-ups:` ideas outside this slice.
6. Report failures plainly. Do not describe a check as passing unless you ran it and read the output.
7. Update the spec tracker (`missions/2026-10-23-foundational-design/spec-tracker.md`) by finding the spec's rows with a search and changing or appending only those lines. The file is over 1,200 lines: never read it whole.
8. After every push, confirm that checks exist on the new head and that `mergeStateStatus` is not `DIRTY` (`gh pr view <PR> --json statusCheckRollup,mergeStateStatus`). An empty check list means untested, not green: a pull request that conflicts with its base runs no CI at all, and the `conflict` label marks it (`specs/094` §12.2.2). Merge the base in, then check again.

## Done when
The diff contains only intended changes, each commit stands alone, and the description's verification claims are all true.

## Pitfalls
- A giant commit titled "fixes".
- Leaving out the test you did not run.
- Pushing to a protected branch or force-pushing.
