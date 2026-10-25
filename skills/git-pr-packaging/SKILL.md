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
5. Write the description: what changed, why, how it was verified (commands and results, including anything skipped), and risks.
6. Report failures plainly. Do not describe a check as passing unless you ran it and read the output.

## Done when
The diff contains only intended changes, each commit stands alone, and the description's verification claims are all true.

## Pitfalls
- A giant commit titled "fixes".
- Leaving out the test you did not run.
- Pushing to a protected branch or force-pushing.
