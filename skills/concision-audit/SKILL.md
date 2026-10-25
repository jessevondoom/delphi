---
name: concision-audit
description: "Audit documents and knowledge-base articles for bloat, redundancy and unverified claims, and cut them. Use when a document is long or restates others."
authorized_agents:
  - writer
input_schema:
  type: object
  required: ["target"]
  properties:
    target:
      type: string
      description: "Document or directory to audit"
output_schema:
  type: object
  required: ["audit"]
  properties:
    audit:
      type: string
      description: "Cuts proposed with reasons and any claims needing a source"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Concision Audit

Nothing in excess. Every line should earn its place or be removed.

## When to use
- A document is long, repeats another, or is loaded into agent context where tokens cost.

## Steps
1. Measure first: line count and size against any cap that applies (skills and manifests 500 lines, rule files 25 rules).
2. Mark repetition: the same fact in two places. Keep it where it belongs and link from the other.
3. Mark claims without a source or that the code no longer supports. Verify or remove them.
4. Cut preamble, restated headings, and examples that teach nothing the text did not.
5. Never cut a caveat, a safety rule, or a requirement for brevity's sake; compress its wording.
6. Report the before and after size and the list of cuts so the change can be reviewed.

## Done when
The document is shorter, says nothing false, and every cut is accounted for.

## Pitfalls
- Compressing until it is cryptic.
- Deleting history that explains why a decision was made.
