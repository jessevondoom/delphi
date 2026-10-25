---
name: trade-off-matrix
description: "Compare options across weighted criteria and test whether the winner survives changed weights. Use when a decision has several defensible answers."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["options"]
  properties:
    options:
      type: array
      items: {type: string}
      description: "Candidate designs, two to five"
    criteria:
      type: array
      items: {type: string}
      description: "What matters, with weights"
output_schema:
  type: object
  required: ["matrix"]
  properties:
    matrix:
      type: string
      description: "Scored matrix, sensitivity note and recommendation"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Trade-off Matrix

A matrix makes the decision criteria explicit so they can be argued about instead of the answer.

## When to use
- Several options are plausible and the choice will be hard to reverse.
- Not when one option is clearly dominated or the decision is cheap to undo.

## Steps
1. Include the status quo or do-nothing as an option, and the simplest thing that could work.
2. Choose criteria that actually differ between options (cost, latency, lock-in, complexity, risk). Drop criteria all options tie on.
3. Weight the criteria before scoring, and say why. Use a small scale such as 1 to 5.
4. Score each cell with a one-phrase justification, using evidence where it exists.
5. Total the scores, then move each weight up and down and see whether the winner changes. Record which weights flip it.
6. Recommend one option. If the result is sensitive to a single weight, say that is the real decision and ask for it.

Use `templates/trade-off-matrix.md` for the layout.

## Done when
The recommendation, its margin, and the weight that would overturn it are all stated.

## Pitfalls
- Weights chosen after seeing which option wins.
- Scores with no reasons.
