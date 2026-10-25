---
name: narrative-briefing
description: "Write plans, proposals and release notes that are specific, honest and free of filler. Use for announcements and personal or project plans."
authorized_agents:
  - writer
input_schema:
  type: object
  required: ["purpose"]
  properties:
    purpose:
      type: string
      description: "What the piece is for"
    facts:
      type: array
      items: {type: string}
      description: "The verified facts it must contain"
output_schema:
  type: object
  required: ["draft"]
  properties:
    draft:
      type: string
      description: "Path of the draft"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Narrative Briefing

Say what is true, what changed, and what happens next, in as few words as it takes.

## When to use
- Release notes, project proposals, status updates, or a personal plan someone will act on.

## Steps
1. Collect the facts first and mark each as verified, inferred or unknown. Write only from the first two.
2. Open with the point: what is changing, what is proposed, or what is decided.
3. Use concrete nouns, numbers and dates. Replace "significantly improved" with the measurement.
4. Give the reader the next action and who owns it.
5. Cut filler: stock openers, intensifiers, hedging that carries no information, and summaries of what you just said.
6. Check every claim against its source, and every link and command by following it.

## Done when
Each sentence is either a fact with a source or a clearly labelled decision, and a reader can act on it.

## Pitfalls
- Claiming a release does something it does not.
- Buzzwords where a number should be.
