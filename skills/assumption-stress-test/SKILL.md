---
name: assumption-stress-test
description: "Run a pre-mortem on a plan, proposal or research: assume it failed and find the likeliest reasons and the cheapest tests of them. Use before committing."
authorized_agents:
  - red-team
input_schema:
  type: object
  required: ["plan"]
  properties:
    plan:
      type: string
      description: "The proposal or conclusion to attack"
output_schema:
  type: object
  required: ["dissent"]
  properties:
    dissent:
      type: string
      description: "Path of the pre-mortem with ranked assumptions and tests"
required_permissions:
  - read:all
  - write:missions
---
# Assumption Stress Test

Imagine it is six months later and this failed. Why?

## When to use
- Before committing to a plan, a design, or a conclusion drawn from research.

## Steps
1. Restate the plan and the outcome it promises.
2. List the assumptions it depends on, including the ones nobody wrote down: about users, data, performance, people, and other systems.
3. Write the failure story: the three most plausible ways it fails, each with a trigger and a consequence.
4. Rank assumptions by how much hangs on them and how little evidence exists.
5. For the top ones, name the cheapest observation or experiment that would confirm or kill the assumption.
6. Record where you disagree with the plan, even if the plan stands. Fill `templates/premortem-dissent.md`.

## Done when
The top assumptions each have a test and an owner, and the author could answer every point or change the plan.

## Pitfalls
- Contrarianism for its own sake: every point needs a scenario.
- Ending without a recommended next step.
