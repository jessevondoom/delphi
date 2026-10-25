---
name: pr-alignment-review
description: "Audit a PR or spec against the operator's goal, the governing specs and the REVIEW.md rubric, mapping acceptance criteria to evidence. Use at review."
authorized_agents:
  - strategist
input_schema:
  type: object
  required: ["change"]
  properties:
    change:
      type: string
      description: "PR, branch or spec under review"
    spec:
      type: string
      description: "The spec it should implement"
output_schema:
  type: object
  required: ["review"]
  properties:
    review:
      type: string
      description: "Findings ranked by severity with evidence"
required_permissions:
  - read:all
  - write:missions
---
# PR Alignment Review

Does this change do what was asked, in the way the specs say, and nothing else?

## When to use
- A PR or spec amendment is ready for review.

## Steps
1. Load `REVIEW.md` and `GOVERNANCE.md` from the target base branch, not from the change under review.
2. Read the linked spec and list its acceptance criteria. Read the diff.
3. For each criterion, find the test or code that satisfies it. Mark it **covered**, **partly**, or **missing**, with `path:line`.
4. List changes that no criterion asked for (see `scope-creep-detection`).
5. Check the rubric's invariants relevant to this diff. Rank findings: blocking, major, minor. Every finding gives the location, the failure scenario and a suggested fix.
6. Say what you did not review. A review that cannot see CI results says so.

## Done when
Every acceptance criterion has a status with evidence, and every finding is concrete enough to act on.

## Pitfalls
- Style opinions presented as blockers.
- Approving on the PR description instead of the diff.
