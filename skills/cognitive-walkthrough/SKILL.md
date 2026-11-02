---
name: cognitive-walkthrough
description: "Perform structured 4-question cognitive walkthroughs on CLI commands, TUI screens, and user workflows to identify mental model mismatches and execution gulfs."
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
output_schema:
  type: object
  required: ["walkthrough_audit"]
  properties:
    walkthrough_audit:
      type: string
      description: "Step-by-step cognitive friction analysis evaluating visibility, affordances, and feedback"
required_permissions:
  - read:all
  - write:missions
---
# Cognitive Walkthrough

Evaluate user interfaces and command workflows step-by-step using the four cognitive walkthrough questions.

## When to use
- Assessing new CLI sub-commands or TUI interaction flows before release.
- Diagnosing why users make mistakes or get stuck during onboarding.
- Auditing the Gulf of Execution and Gulf of Evaluation in complex multi-step tools.

## Steps
1. **Define User Persona & Goal:** Ground the evaluation in a specific operator goal.
2. **Execute Step-by-Step Analysis:** At each discrete user action, answer:
   - *Will the user try to achieve the right effect?*
   - *Will the user notice that the correct action is available?*
   - *Will the user associate the correct action with the intended effect?*
   - *If the correct action is performed, will the user see that progress is being made?*
3. **Compile Friction Points:** Document every failure point where the user is likely to hesitate, make an error, or experience confusion.
4. **Recommend Affordance Fixes:** Provide concrete terminal/CLI adjustments (better defaults, clearer feedback, tab completion hints).

## Output format
- **Goal Statement:** User objective.
- **Walkthrough Matrix:** Question-by-question evaluation for each step.
- **Recommended Ergonomic Fixes:** Specific visual and behavioral enhancements.
