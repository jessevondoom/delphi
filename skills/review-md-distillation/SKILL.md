---
name: review-md-distillation
description: "Update REVIEW.md and heuristics from review outcomes within their budgets; flag rules violated twice for a deterministic check. Use after review rounds."
authorized_agents:
  - orchestrator
input_schema:
  type: object
  required: ["findings"]
  properties:
    findings:
      type: string
      description: "Review findings or retrospective note to learn from"
output_schema:
  type: object
  required: ["proposal"]
  properties:
    proposal:
      type: string
      description: "Path of the proposed edit"
    promotions:
      type: array
      items: {type: string}
      description: "Rules to turn into checks"
required_permissions:
  - read:all
  - write:missions
---
# REVIEW.md Distillation

The rubric stays short by moving enforceable rules into code. Prose is for judgement.

## When to use
- A review pattern recurs, or REVIEW.md is approaching its line budget.

## Steps
1. Count how often each finding repeated across recent reviews. Once is an observation; twice is a rule violated twice.
2. A rule violated twice is a candidate for a deterministic check: propose it to Builder (a Red-tier change) and, once the check exists, remove the prose rule.
3. For a new judgement rule, write it in one line with an ID, the failure it prevents, and the evidence.
4. Merge or drop rules the evidence no longer supports.
5. Write the edit as a proposal; the rubric changes only through ratification.
6. Run `delphi check review-md REVIEW.md` against the proposed text. It enforces the line budget and unique rule IDs.

No shell? Call the `check` tool with `kind: review-md` and the proposed file's path; it returns the same verdict.

## Done when
The proposal is shorter or equal in size, each added rule cites evidence, and each promotion names the check that replaces it.

## Pitfalls
- Adding a rule without removing one.
- Promoting a finding from an external source without verification.
