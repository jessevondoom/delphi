---
name: risk-roi-analysis
description: "Weigh the effort and risk of an option against its expected payoff and the cost of not doing it, using a one-page brief. Use when deciding whether to invest."
authorized_agents:
  - strategist
input_schema:
  type: object
  required: ["option"]
  properties:
    option:
      type: string
      description: "The work or plan under consideration"
    alternatives:
      type: array
      items: {type: string}
      description: "What else the effort could buy"
output_schema:
  type: object
  required: ["brief"]
  properties:
    brief:
      type: string
      description: "Path of the risk and ROI brief"
required_permissions:
  - read:all
  - write:missions
---
# Risk and ROI Analysis

Decide whether the work is worth doing now, before deciding how.

## When to use
- A proposal asks for significant effort and the payoff is uncertain.

## Steps
1. State the payoff in observable terms: time saved, failures avoided, capability unlocked. Say how you would notice it.
2. Estimate effort as a range, including review, testing, and the cost of maintaining the result.
3. List the main risks with a rough likelihood and impact, and what would reduce each.
4. Name the opportunity cost: what the same effort would otherwise deliver.
5. Include the cheapest experiment that would reduce the largest uncertainty.
6. Recommend: do now, do later, do smaller, or do not do. Fill `templates/risk-roi-brief.md`.

## Done when
The brief gives a recommendation and states which assumption, if wrong, would reverse it.

## Pitfalls
- Precise-looking numbers for guesses: keep ranges.
- Counting benefits that need other work that is not planned.
