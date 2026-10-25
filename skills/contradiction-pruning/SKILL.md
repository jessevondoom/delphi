---
name: contradiction-pruning
description: "Find stale claims, broken links and superseded documents in the knowledge base and propose fixes. Use periodically and before a release."
authorized_agents:
  - orchestrator
input_schema:
  type: object
  required: ["root"]
  properties:
    root:
      type: string
      description: "Directory to audit, usually the repository or workspace root"
output_schema:
  type: object
  required: ["report"]
  properties:
    report:
      type: string
      description: "Path of the audit report"
    fixes:
      type: array
      items: {type: string}
      description: "Proposed edits"
required_permissions:
  - read:all
  - write:missions
---
# Contradiction Pruning

A knowledge base that disagrees with itself teaches agents the wrong thing.

## When to use
- Before a release, after a spec is superseded, or when two documents seem to disagree.

## Steps
1. Run `delphi check kb <root>`. It reports broken links, bad provenance frontmatter, rule-cap violations, and live documents that still link to superseded ones.
2. Triage each finding: fix the link, update the claim, or mark the document `superseded` with `superseded_by`.
3. Then read for contradictions the tool cannot see: two documents that state different values for the same thing. Quote both with `path:line`, and decide which governs using the hierarchy (GOVERNANCE, then specs and REVIEW.md, then implementation).
4. Record the fixes as proposals for anything under ratification; apply direct fixes only where your role may write.

## Done when
`delphi check kb` is clean and every remaining contradiction is listed with both locations and the proposed winner.

## Pitfalls
- Deleting a superseded document instead of marking it.
- Resolving a contradiction by editing the lower-tier document to match the higher one without telling anyone.
