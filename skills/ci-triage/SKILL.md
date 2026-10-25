---
name: ci-triage
description: "Diagnose a failing CI or test run: separate real regressions from flakes and environment problems, and say what to do. Use when a pipeline is red."
authorized_agents:
  - builder
input_schema:
  type: object
  required: ["run"]
  properties:
    run:
      type: string
      description: "Failing run, log path, or command"
    change:
      type: string
      description: "The diff or branch under test"
output_schema:
  type: object
  required: ["diagnosis"]
  properties:
    diagnosis:
      type: string
      description: "Classification, evidence, and the next action"
required_permissions:
  - read:all
  - shell:scoped_scripts
---
# CI Triage

Red does not mean broken. Classify it before acting.

## When to use
- A CI job or local test run failed and the cause is not obvious.

## Steps
1. Find the first failure, not the last. Strip the noise (colour codes, progress output) and keep the failing test name, the assertion, and the stack frames in the project's code.
2. Ask whether the failure relates to the change. Does the failing code path touch files in the diff?
3. Rerun the failing test alone, then the job. Record the result of each run.
4. Classify: **regression** (fails consistently, related to the change), **flake** (passes on rerun, with a plausible race or timing cause), **environment** (runner, cache, network, timeout), or **pre-existing** (fails on the base branch too).
5. For a suspected flake, find the race and say what it is; a flake that shipped once was sometimes a real bug. Do not call it a flake on a single pass.
6. State the action: fix, rerun, quarantine with an issue, or escalate. Merge conflicts with the base can suppress CI entirely; check the branch is current.

## Done when
The diagnosis names a class, cites the log lines and reruns behind it, and gives one next action.

## Pitfalls
- Rerunning until green without reading the failure.
- Waiting on a hung job instead of rerunning it.
