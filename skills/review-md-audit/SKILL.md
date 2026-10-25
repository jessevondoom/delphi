---
name: review-md-audit
description: "Audit a diff strictly against the base branch's REVIEW.md, never the PR's own copy, and report findings with failure scenarios. Use when reviewing a PR."
authorized_agents:
  - red-team
input_schema:
  type: object
  required: ["change"]
  properties:
    change:
      type: string
      description: "Diff, branch or PR to audit"
    base:
      type: string
      description: "Base ref, usually origin/main"
output_schema:
  type: object
  required: ["findings"]
  properties:
    findings:
      type: array
      items: {type: string}
      description: "Findings with location, rule, failure scenario and fix"
required_permissions:
  - read:all
  - shell:scoped_scripts
---
# REVIEW.md Audit

A change cannot be allowed to rewrite the rules it is judged by.

## When to use
- Reviewing any PR or spec amendment.

## Steps
1. Load the rubric from the base branch (`git show origin/main:REVIEW.md`, or the local base ref when there is no remote), and the same for `GOVERNANCE.md`. If neither is available, fail closed and say so.
2. If the diff itself edits `REVIEW.md` or `GOVERNANCE.md`, review those edits against the base version of the rules; the new text does not apply to itself.
3. Read the diff against each rule that could apply. Start with security and sovereignty invariants, then spec alignment, then correctness.
4. For every finding give: the rule ID, the `path:line`, a concrete failure scenario (inputs or state, then what goes wrong), and a remediation. A finding without a scenario is an opinion.
5. Rank by severity as the rubric defines it and respect the round cap: do not reopen resolved findings without new evidence.
6. State what you did not check.

## Done when
Every finding is falsifiable and ranked, and the rubric version you used is named.

## Pitfalls
- Loading the rubric from the branch under review.
- Findings that cannot be disproved.
