---
name: council-moderation
description: "Run the three-round Council: blind Round 1, cross-examination, Verdict plus verbatim Minority Report. Use when a decision needs adversarial review."
authorized_agents:
  - orchestrator
input_schema:
  type: object
  required: ["question"]
  properties:
    question:
      type: string
      description: "The decision to deliberate"
    seats:
      type: array
      items: {type: string}
      description: "Roles to seat, three to five"
output_schema:
  type: object
  required: ["record"]
  properties:
    record:
      type: string
      description: "Path of the Council record under the mission"
required_permissions:
  - read:all
  - write:missions
---
# Council Moderation

A Council exists to surface disagreement, not to produce consensus. The moderator keeps the protocol honest.

## When to use
- An architecture, scope or risk decision with real trade-offs, or any Red-tier proposal.
- Not for questions with a checkable answer: look it up.

## Steps
1. State the question and the decision criteria in two or three sentences. Seat three to five roles with different perspectives, spread across model families where more than one is configured.
2. **Round 1 (blind).** Each seat writes its stance in isolation. No seat sees another until every Round 1 stance is written.
3. **Round 2 (cross-examination).** Each seat reads all stances and attacks the others from its own angle: Red-Team on failure modes, Strategist on scope and payoff, Builder on buildability, Architect on structure.
4. **Round 3 (verdict).** Write the verdict, the dissent that remains, and the Minority Report verbatim, not paraphrased. Stop at three rounds; a deadlock goes to the operator.
5. Save the record under `missions/<mission>/councils/NNN-<name>.md` with provenance frontmatter.
6. Run `delphi lint --repo .`: it verifies that a runtime-written record's Minority Report matches its Round 2 output.

## Done when
The verdict names what was decided and why, every unresolved objection appears in the Minority Report in the objector's words, and lint passes.

## Pitfalls
- Smoothing dissent into "broad agreement".
- Letting a seat read peers early.
- A fourth round: more rounds drift toward agreement, not truth.
