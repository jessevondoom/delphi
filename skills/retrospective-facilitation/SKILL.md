---
name: retrospective-facilitation
description: "Run a short post-mission retrospective: what cost turns, what repeated, what to change. Use at mission close, before compaction."
authorized_agents:
  - orchestrator
input_schema:
  type: object
  required: ["mission"]
  properties:
    mission:
      type: string
      description: "Mission id to review"
output_schema:
  type: object
  required: ["retro"]
  properties:
    retro:
      type: string
      description: "Path of the retrospective note"
    candidates:
      type: array
      items: {type: string}
      description: "Proposed lessons or script promotions"
required_permissions:
  - read:all
  - write:missions
---
# Retrospective Facilitation

Find the few things that cost the most and change how the team works next time.

## When to use
- A mission is closing, or an agent hit its turn budget or needed repeated retries.

## Steps
1. Gather the facts first: turns used, retries, review rounds, escalations, and failed gates per task, from the mission log. Do not rely on memory.
2. Ask each involved role (Strategist leads) three things: what slowed it down, what it would do differently, what rule or tool was missing.
3. Separate causes from symptoms. A flaky test is a symptom; a missing isolation rule is a cause.
4. Pick at most three changes. For each, say whether it is a lesson (`lesson-promotion`), a rule needing a deterministic check (two strikes), or a spec change.
5. Write `missions/<mission>/retro.md`: facts, causes, the chosen changes, and who owns each.

## Done when
Every proposed change names an owner and the evidence behind it, and nothing is promoted without verification.

## Pitfalls
- A list of ten lessons: the cap is deliberate.
- Promoting an unverified finding into always-loaded memory.
