---
name: persona-simulation
description: "Model goal-directed user personas, Jobs to Be Done (JTBD) Job Stories, and cognitive walkthrough scenarios. Use when evaluating product ergonomics, specs, or UI designs."
authorized_agents:
  - advocate
input_schema:
  type: object
  required: ["feature_or_workflow"]
  properties:
    feature_or_workflow:
      type: string
      description: "Proposed capability, CLI command, or user workflow to evaluate"
    user_context:
      type: string
      description: "Target user archetype and environmental constraints"
output_schema:
  type: object
  required: ["job_story", "persona_walkthrough"]
  properties:
    job_story:
      type: string
      description: "Standard JTBD Job Story formula defining motivation and expected progress"
    persona_walkthrough:
      type: string
      description: "Cognitive step-by-step evaluation simulating user mental models and potential friction"
required_permissions:
  - read:all
  - write:missions
---
# Persona Simulation

Simulate realistic, goal-directed personas and evaluate workflows against user mental models.

## When to use
- Reviewing new specs, CLI commands, or UI screens for usability and cognitive friction.
- Identifying edge-case operator workflows and unhandled mental model mismatches.
- Framing product capabilities around genuine user progress rather than engineering internals.

## Steps
1. **Formulate Goal-Directed Persona:** Define Life Goals (identity), End Goals (concrete outcomes), and Experience Goals (affective state).
2. **Draft the JTBD Job Story:** State: *"When [context], I want to [action], so I can [progress]."*
3. **Run 4-Question Cognitive Walkthrough:** At each workflow step, assess:
   - *Will the user try to achieve the right effect?*
   - *Will the user notice the correct action is available?*
   - *Will the user associate the action with the intended effect?*
   - *Will the user perceive that progress was made?*
4. **Deliver Ergonomic Critique:** Highlight cognitive friction points and recommend friction-reducing affordances.

## Output format
- **Target Persona Profile:** Goals and mental model.
- **Job Story:** Concrete human motivation.
- **Walkthrough Friction Points:** Specific UX hurdles and proposed simplifications.
