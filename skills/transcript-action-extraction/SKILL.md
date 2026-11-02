---
name: transcript-action-extraction
description: "Forensically extract actionable commitments from meeting and conversation transcripts, eliminating hallucinated consensus and enforcing TRIAD ownership. Use when digesting audio/video call notes."
authorized_agents:
  - assistant
input_schema:
  type: object
  required: ["transcript"]
  properties:
    transcript:
      type: string
      description: "Raw or timestamped conversation/meeting transcript"
output_schema:
  type: object
  required: ["actions", "decisions", "unresolved_concerns"]
  properties:
    actions:
      type: array
      items:
        type: object
        required: ["owner", "deliverable", "due_date"]
        properties:
          owner: {type: string}
          deliverable: {type: string}
          due_date: {type: string}
    decisions:
      type: array
      items: {type: string}
    unresolved_concerns:
      type: array
      items: {type: string}
required_permissions:
  - read:all
  - write:missions
---
# Transcript Action Extraction

Forensically extract commitments, decisions, and dissent from conversation transcripts without hallucinating consensus.

## When to use
- Summarizing recorded meetings, Zoom calls, or voice transcripts.
- Converting discussion notes into tracked project tasks.
- Auditing multi-stakeholder alignments for silent disagreements.

## Steps
1. **Filter Brainstorming from Decisions:** Differentiate speculative banter from explicit policy decisions.
2. **Reject Passive Agreement:** Do not log casual politeness as organizational signoff. Require explicit verbal commitment.
3. **Enforce the TRIAD:** Every action item must strictly define:
   - **Owner:** The explicit individual responsible.
   - **Deliverable:** Concrete, verifiable output.
   - **Due Date:** Specific timeline or milestone.
4. **Preserve Dissent:** Explicitly log quiet hesitations or objections under `unresolved_concerns`.

## Output format
- **Decisions Ratified:** Explicit consensus points.
- **Action Registry:** Table of TRIAD-compliant commitments.
- **Unresolved Dissent:** Documented objections and open loops.
