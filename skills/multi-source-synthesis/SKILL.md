---
name: multi-source-synthesis
description: "Combine notes and sources into one compact landscape map where every claim points to its source. Use when several inputs must become one picture."
authorized_agents:
  - scout
input_schema:
  type: object
  required: ["inputs"]
  properties:
    inputs:
      type: array
      items: {type: string}
      description: "Notes, maps or sources to combine"
    question:
      type: string
      description: "What the synthesis must answer"
output_schema:
  type: object
  required: ["synthesis"]
  properties:
    synthesis:
      type: string
      description: "Path of the landscape map under 300 lines"
required_permissions:
  - read:all
  - write:missions
---
# Multi-source Synthesis

A synthesis is shorter than its inputs and traceable back to them.

## When to use
- Several scouts, surveys or documents need to become one understanding.

## Steps
1. List the inputs and note each one's trust tier. The result is no more trusted than its weakest essential input.
2. Extract claims into a plain list, each with its source pointer.
3. Group claims into themes. Within a theme, mark where sources agree, where they conflict, and where only one speaks.
4. Resolve conflicts only where the evidence supports it; otherwise present both and say what would decide.
5. Write the map under 300 lines. Every statement carries a pointer; a statement without one is deleted or marked as inference.
6. Close with gaps: questions no input answers.

## Done when
Any sentence can be traced to a source, and conflicts remain visible rather than averaged away.

## Pitfalls
- Smoothing disagreement into a single confident voice.
- Adding your own claims without marking them as inference.
