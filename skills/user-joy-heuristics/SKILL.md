---
name: user-joy-heuristics
description: "Audit software interfaces, TUI screens, and CLI interactions against emotional design levels, Kahneman's Peak-End Rule, and Nielsen's heuristics to maximize user joy and eliminate friction."
authorized_agents:
  - advocate
input_schema:
  type: object
  required: ["screen_or_interaction"]
  properties:
    screen_or_interaction:
      type: string
      description: "CLI command output, TUI layout, error message, or workflow interaction sequence"
output_schema:
  type: object
  required: ["joy_audit"]
  properties:
    joy_audit:
      type: string
      description: "Structured evaluation across Visceral, Behavioral, and Reflective levels with concrete joy enhancements"
required_permissions:
  - read:all
  - write:missions
---
# User Joy Heuristics

Evaluate and optimize interfaces for emotional resonance, effortless mastery, and user joy.

## When to use
- Designing or reviewing terminal layouts, prompts, and output styling.
- Polishing error messages, onboarding sequences, and completion summaries.
- Auditing user experience against Kahneman's Peak-End Rule and Norman's emotional design levels.

## Steps
1. **Visceral Assessment:** Evaluate visual aesthetics, alignment, typography, and contrast. Is it visually balanced and appealing at first glance?
2. **Behavioral Assessment:** Assess operational speed, latency feedback, and keystroke ergonomics. Does interaction feel effortless, competent, and fluid?
3. **Reflective Assessment:** Evaluate the post-task emotional resonance. Does the user feel accomplished, in control, and proud of the work?
4. **Peak-End Audit:** Ensure the conclusion provides satisfying closure and that errors are paired with empathetic, helpful recovery actions.
5. **Accessibility & Motion Check:** Verify adherence to `/reduce-motion` and high-contrast accessibility standards.

## Output format
- **Aesthetic / Visceral Polish:** Visual hierarchy and layout refinements.
- **Operational / Behavioral Enhancements:** Latency feedback, key shortcuts, and default actions.
- **Joy & Ending Polish:** Peak-End touches, celebratory completions, and recovery steps.
