---
name: tonal-calibration
description: "Calibrate and shift prose across technical, executive, persuasive, and educational registers for reader comfort. Use when adapting documentation, mission briefs, or reports for distinct audiences."
authorized_agents:
  - writer
input_schema:
  type: object
  required: ["content"]
  properties:
    content:
      type: string
      description: "Source draft, technical notes, or briefing text to calibrate"
    target_register:
      type: string
      description: "Desired register: technical-reference, executive-brief, persuasive-proposal, or onboarding-guide"
output_schema:
  type: object
  required: ["calibrated_text"]
  properties:
    calibrated_text:
      type: string
      description: "Transformed prose aligned with the target register, reading level, and cognitive profile"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Tonal Calibration

Calibrate communication registers and dynamically code-switch across technical, executive, persuasive, and pedagogical tones.

## When to use
- Adapting deep technical implementation details into an executive summary or operator decision brief.
- Translating dense research findings into high-clarity onboarding guides or architectural reference manuals.
- Smoothing combative or overly terse agent debate output into constructive, collaborative documentation.

## Steps
1. **Analyze Source Content & Density:** Identify core technical payload, domain vocabulary, logical flow, and current register.
2. **Determine Target Register & Audience Profile:**
   - *Technical Reference:* Dense, exact, invariant-focused, unambiguous, high symbol-to-word ratio.
   - *Executive Brief:* Outcome-oriented, risk/ROI highlighted, minimal implementation details, decisive recommendations.
   - *Persuasive Proposal:* Problem-first, clear value proposition, addressed objections, structured trade-offs.
   - *Pedagogical / Onboarding:* Empathetic, conceptual scaffolding, concrete examples, progressive disclosure.
3. **Calibrate Vocabulary & Syntax:** Adjust sentence length, remove bureaucratic jargon, eliminate cognitive filler words, and tune passive vs. active voice.
4. **Preserve Technical Invariants:** Ensure semantic precision is never sacrificed for superficial readability. Fact-check transformed metrics and code symbols against primary sources.
5. **Verify Tone & Brevity:** Run a concision sweep. Check that the opening sentence establishes immediate context and the concluding paragraph states clear next steps.
6. **Final Calibration Check:** Ensure the calibrated draft adheres to Delphi's documentation standards and zero-filler principle.

## Guidelines
- **Zero filler:** Simple tone does not mean fluff. Avoid cheerleading, buzzwords, or patronizing conversational pleasantries.
- **Invariant preservation:** Never blur distinct technical concepts in the name of simplicity.
- **Audience respect:** Match the cognitive load to the reader's role; respect operator time above all else.

## Output format
A calibrated document containing:
- **Register Specification:** Target audience and selected register parameters.
- **Calibrated Prose:** Full text rewritten in the calibrated register.
- **Invariant Preservation Audit:** Checklist proving technical facts and terms were accurately preserved.
- **Readability Metrics:** Word count change, reading level, and concision delta.
