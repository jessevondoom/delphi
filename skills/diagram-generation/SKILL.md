---
name: diagram-generation
description: "Produce a Mermaid diagram that shows real structure or flow, with quoted labels and the right diagram type. Use when a picture beats prose."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["subject"]
  properties:
    subject:
      type: string
      description: "What to depict"
    kind:
      type: string
      description: "flowchart, sequenceDiagram or stateDiagram-v2"
output_schema:
  type: object
  required: ["diagram"]
  properties:
    diagram:
      type: string
      description: "The Mermaid source and where it was placed"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Diagram Generation

Draw a diagram only when it carries something prose cannot: order, branching, ownership, or state.

## When to use
- A flow with branches, a protocol between actors, or a state machine.
- Not for a list: write the list.

## Steps
1. Decide the one question the diagram answers. If you need two, draw two.
2. Choose the type: `flowchart` for control or data flow, `sequenceDiagram` for messages over time, `stateDiagram-v2` for lifecycles.
3. Name nodes after the real components, files or states in the code and specs. Label edges with what flows.
4. Quote every label that contains punctuation or spaces. Avoid reserved words as ids (`end`, `graph`).
5. Keep it to roughly a dozen nodes; split anything bigger.
6. Check the syntax by rendering it if a renderer is available, or re-read it line by line. Place it in a fenced `mermaid` block next to the text it supports.

## Done when
It renders, every node maps to something real, and the surrounding text does not need to repeat it.

## Pitfalls
- Decorative diagrams of boxes and arrows with no information.
- Diagrams that go stale: link them to the spec section they illustrate.
