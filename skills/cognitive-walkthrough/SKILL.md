---
name: cognitive-walkthrough
description: "Name a persona and job story, then walk a command, screen or workflow with the four walkthrough questions. Findings are simulated, not observed. Use when reviewing usability before release."
authorized_agents:
  - advocate
input_schema:
  type: object
  required: ["workflow_steps"]
  properties:
    workflow_steps:
      type: array
      items: {type: string}
      description: "Ordered sequence of user actions and expected system responses"
    feature_or_workflow:
      type: string
      description: "The capability, CLI command or workflow the steps belong to"
    user_context:
      type: string
      description: "Target user archetype and environmental constraints"
output_schema:
  type: object
  required: ["walkthrough_audit"]
  properties:
    job_story:
      type: string
      description: "The JTBD job story: when [context], I want to [action], so I can [progress]"
    walkthrough_audit:
      type: string
      description: "Step-by-step friction analysis; each finding labelled `simulated, not observed` with the screen, output or snapshot it was read from"
required_permissions:
  - read:all
  - write:missions
---
# Cognitive Walkthrough

Simulate one person with one goal moving through a workflow, and ask at each step whether they would get it right. This skill carries what `persona-simulation` used to (`specs/124` §4): the persona and the job story are its first two steps.

No user is watched here. A walkthrough predicts where a person may stumble; it does not show that they do.

## When to use
- Reviewing a new spec, CLI sub-command or console screen for usability before release.
- Working out why people make a mistake or get stuck during onboarding.
- Framing a capability around the progress a person wants, not the engineering inside it.

## Steps
1. **Name the persona:** one person, with an end goal (the concrete outcome), an experience goal (how they want to feel) and what they already know. Say which parts you assumed.
2. **Write the job story:** *"When [context], I want to [action], so I can [progress]."*
3. **Find the evidence:** for each step, the real thing a person would see: the command's actual output, the screen, or its golden snapshot (`ui/test/snapshots*`). Read it; do not walk through a screen from memory.
4. **Ask the four questions at each step:**
   - *Will the user try to achieve the right effect?*
   - *Will the user notice that the correct action is available?*
   - *Will the user associate the correct action with the intended effect?*
   - *If the correct action is performed, will the user see that progress is being made?*
5. **Write each finding:** the step, the question it failed, why, the path of the screen, output or snapshot it was read from, and the label `simulated, not observed`.
6. **Recommend the fix:** a concrete change (a better default, clearer feedback, a completion hint), and what a test with a real person would have to show.

## Output format
- **Persona and Job Story:** the person, their goal, your assumptions.
- **Walkthrough Matrix:** each step against the four questions.
- **Findings:** each with its evidence path and `simulated, not observed`.
- **Recommended Fixes:** specific changes, in order of how much friction they remove.

## Done when
Every finding names a step, a failed question and a path you read, carries the label, and no finding rests on a screen you did not open.

## Pitfalls
- Reporting a simulation as what users do ("users are confused by"): write "a first-time user would likely".
- A persona who conveniently knows what the designer knows.
- Walking the happy path only: include the first error a person would hit.
- A finding with no screen to point at.
