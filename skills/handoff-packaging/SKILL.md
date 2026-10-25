---
name: handoff-packaging
description: "Package a handoff as a pointer-based envelope: objective, file:line context, constraints, expected output. Use before dispatching any agent."
authorized_agents:
  - orchestrator
input_schema:
  type: object
  required: ["objective"]
  properties:
    objective:
      type: string
      description: "What the receiving agent must produce and why"
    target:
      type: string
      description: "Receiving role"
    sources:
      type: array
      items: {type: string}
      description: "Files and line ranges that matter"
output_schema:
  type: object
  required: ["envelope"]
  properties:
    envelope:
      type: string
      description: "Path of the validated handoff envelope"
required_permissions:
  - read:all
  - write:missions
---
# Handoff Packaging

An agent never receives a transcript. It receives a short envelope that points at what it needs.

## When to use
- Every dispatch from Orchestrator, and every agent-to-agent request.

## Steps
1. Write the objective as the artifact wanted and the reason, in a few sentences.
2. Add `context_pointers`: for each, a workspace-relative `uri` with a line anchor (`specs/002-x.md#L40-L72`), the `heading` it falls under, a `blake3` of the content, and a one-line `reason` it matters. Point at the smallest range that answers the need.
3. List constraints that cannot be violated (spec clauses, REVIEW.md rules) and name the `expected_output.artifact_type`.
4. Set `routing_directive.model_tier` if the default is wrong for this task.
5. Start from `templates/handoff-envelope.yaml`, save under `missions/<mission>/handoffs/`.
6. Run `delphi check handoff <envelope>`. It enforces the required fields, the anchors, and the size cap (about 2,000 tokens).

No shell? Call the `check` tool with `kind: handoff` and the envelope's path; it returns the same verdict.

## Done when
The check passes and a reader who has not seen this conversation could do the task from the envelope and the files it points at.

## Pitfalls
- Pasting file contents instead of pointing at them.
- Whole-file pointers: they defeat the cap and bury the relevant lines.
- Stuffing history into the objective.
