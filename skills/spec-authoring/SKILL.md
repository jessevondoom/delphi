---
name: spec-authoring
description: "Write a modular spec with scope, acceptance criteria, a Simpler Alternative Considered section and a verification plan. Use before any non-trivial build."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["problem"]
  properties:
    problem:
      type: string
      description: "What needs specifying and why"
    number:
      type: string
      description: "Spec number and short name"
output_schema:
  type: object
  required: ["spec"]
  properties:
    spec:
      type: string
      description: "Path of the draft spec under specs/"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Spec Authoring

A spec is the contract Builder implements and Red-Team attacks. It should be short enough to be read whole.

## When to use
- New features, architecture changes, or anything a Builder could get wrong by guessing.
- Not for a typo or a one-line fix.

## Steps
1. State the problem and the non-goals first. If you cannot say what is out of scope, the scope is not understood.
2. Read the existing specs that touch the area and link them; do not restate them.
3. Write the design at the level a Builder needs: interfaces, data shapes, invariants, failure behaviour.
4. Write acceptance criteria as observable, testable statements, numbered so tests and PRs can cite them.
5. Write **Simpler Alternative Considered**: the cheapest thing that might work, and the concrete reason it does not. If you cannot find a reason, build the simpler thing.
6. Write the **Verification Plan**: the tests and checks that will prove each criterion.
7. Start from `templates/spec-skeleton.md`, with provenance frontmatter and `status: draft`. An agent never marks a spec `approved`.
8. Run `delphi check spec <file>`.

## Done when
The check passes, every criterion is testable, and the simpler alternative is a real alternative.

## Pitfalls
- Designing the abstraction before the second use case exists.
- Acceptance criteria that restate the design.
- Silent scope growth: put new ideas in a follow-up list.
