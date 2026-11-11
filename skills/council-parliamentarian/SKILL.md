---
name: council-parliamentarian
description: "Audit multi-agent Council debates for procedural compliance with REVIEW.md, extract irreconcilable contentions, and structure operator tie-breaker cards. Use when council sessions reach deadlock."
locus: inside
authorized_agents:
  - administrator
input_schema:
  type: object
  required: ["council_records"]
  properties:
    council_records:
      type: string
      description: "Transcripts or positions from Round 1 and Round 2 council deliberations"
output_schema:
  type: object
  required: ["contention_summary", "decision_card"]
  properties:
    contention_summary:
      type: string
      description: "Neutral distillation of the irreducible architectural or strategic conflict"
    decision_card:
      type: object
      description: "Structured choice card formatted for operator decision"
required_permissions:
  - read:all
  - write:missions
---
# Council Parliamentarian

Serve as neutral procedural referee during Council debates, auditing arguments against `REVIEW.md` and structuring operator tie-breakers.

## When to use
- When Council deliberations split 50/50 across seats.
- When cross-examination circularity prevents consensus.
- When evaluating whether critiques satisfy falsifiability standards.

## Steps
1. **Procedural Review:** Audit each seat's arguments against `REVIEW.md` criteria (scope, simplicity, falsifiability).
2. **Filter Rhetoric & Speculation:** Discard non-falsifiable critiques and speculative requirement expansions.
3. **Isolate Irreducible Trade-Off:** Identify the exact architectural or strategic compromise dividing the seats.
4. **Draft Operator Tie-Breaker Card:** Formulate a structured choice card presenting concrete options and respective blast radii. Never cast an autonomous vote.

## Output format
- **Procedural Findings:** Adherence to rubrics.
- **Contention Ledger:** Irreducible conflict points.
- **Operator Decision Card:** Explicit options ready for operator determination.
