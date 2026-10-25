---
name: scope-creep-detection
description: "Flag changes that go beyond the approved spec: unrequested files, features, dependencies or refactors. Use on every spec and PR before approval."
authorized_agents:
  - strategist
input_schema:
  type: object
  required: ["change"]
  properties:
    change:
      type: string
      description: "Diff or document under audit"
    spec:
      type: string
      description: "The approved spec that defines scope"
output_schema:
  type: object
  required: ["audit"]
  properties:
    audit:
      type: string
      description: "In-scope and out-of-scope items with evidence"
required_permissions:
  - read:all
  - write:missions
---
# Scope Creep Detection

Good ideas outside the spec are still outside the spec.

## When to use
- Before approving a spec for build, and again before merging the PR that implements it.

## Steps
1. Extract the in-scope list from the spec: its goals and acceptance criteria. Extract the non-goals too.
2. Walk the diff by file. For each hunk ask which criterion it serves.
3. Mark hunks that serve none: new dependencies, new options, unrelated cleanup, extra abstraction, speculative extensibility.
4. Separate **harmless** (a tiny rename in a touched line) from **expansion** (new behaviour, new files, new surface).
5. For each expansion, recommend: remove, move to a follow-up, or amend the spec through its owner.
6. Report the ratio: lines or files serving the spec versus not.

## Done when
Every out-of-scope item is listed with its location and a disposition, and the in-scope remainder maps to the criteria.

## Pitfalls
- Letting a refactor ride along because it is "small".
- Treating missing scope as the only failure: a change that skips a criterion also drifts.
