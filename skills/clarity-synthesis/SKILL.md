---
name: clarity-synthesis
description: "Turn dense research, specs or logs into a clear brief for a human reader: the answer, the reasons, the decisions needed. Use when handing findings to the operator."
authorized_agents:
  - writer
input_schema:
  type: object
  required: ["source"]
  properties:
    source:
      type: array
      items: {type: string}
      description: "Documents to distil"
    reader:
      type: string
      description: "Who will read it and what they must decide"
output_schema:
  type: object
  required: ["brief"]
  properties:
    brief:
      type: string
      description: "Path of the brief"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Clarity Synthesis

The reader should know the answer after the first paragraph and be able to check it after the last.

## When to use
- Findings, a spec, or a long thread must be understood by someone who was not there.

## Steps
1. Identify the reader's decision or question. Everything in the brief serves it.
2. Lead with the conclusion and what is needed from the reader, if anything.
3. Give the reasons in order of importance, each with a pointer to the evidence.
4. State uncertainty plainly: what is known, what is inferred, what is unknown.
5. Keep caveats from the sources. Never drop a Red-Team caveat or a Minority Report to make the story smoother.
6. Use `templates/executive-brief.md` for the layout, and cut every sentence the reader would not miss.

## Done when
A reader who stops after the first section still has the right conclusion and next step, and no source's warnings were lost.

## Pitfalls
- Polish over truth: a clear brief of a wrong claim is worse than a rough one of a true claim.
- Describing planned work as done.
