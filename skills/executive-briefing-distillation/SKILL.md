---
name: executive-briefing-distillation
description: "Distill complex operational context, incidents, and multi-channel communications into top-down BLUF briefings and binary decision cards. Use when packaging daily briefs or unblocking decisions."
authorized_agents:
  - assistant
input_schema:
  type: object
  required: ["raw_context"]
  properties:
    raw_context:
      type: string
      description: "Unstructured operational notes, incident logs, or thread summaries"
output_schema:
  type: object
  required: ["bluf_brief"]
  properties:
    bluf_brief:
      type: string
      description: "Structured briefing adhering to BLUF and the Minto Pyramid Principle"
required_permissions:
  - read:all
  - write:missions
---
# Executive Briefing Distillation

Synthesize complex context into high-signal executive briefings adhering to the BLUF (Bottom Line Up Front) doctrine and Minto Pyramid Principle.

## When to use
- Preparing daily operational standup briefs or weekly mission summaries.
- Packaging critical incidents or trade-offs for rapid operator decisions.
- Synthesizing sprawling cross-channel updates into minimal-friction actionable digests.

## Steps
1. **Extract the Core Insight:** Identify the single most important development or required decision.
2. **Bottom Line Up Front:** State the primary finding or recommendation in the first sentence.
3. **Minto Hierarchy:** Organize supporting arguments into MECE (Mutually Exclusive, Collectively Exhaustive) bullet points.
4. **Action Formulation:** Provide explicit binary choices (Option A vs. Option B) with trade-offs.

## Output format
- **BLUF:** 1-sentence bottom line.
- **Key Findings:** 3 structured bullet points.
- **Action Required:** Concrete operator choice.
