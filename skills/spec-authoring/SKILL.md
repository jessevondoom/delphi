---
name: spec-authoring
description: "Write a modular spec with scope, acceptance criteria, Simpler Alternative Considered, Options considered and a verification plan. Use before any non-trivial build."
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
6. Write **Options considered**, beside it and not in its place: load `divergent-options` and record at least three options (one conventional, one that removes a named constraint, one by a named method), each with one sentence on how it would be tested, and which one was chosen. The simpler-alternative rule of step 5 is unchanged: this section does not satisfy it, and it still has to survive review by Builder, Strategist and Red-Team.
7. Write the **Verification Plan**: the tests and checks that will prove each criterion, by number.
   - **Slices** (`specs/133` §2.3): each closes named criteria, is observable by a test, and is about 300 changed lines or fewer. Past about 8 slices, or at a second amendment, split into a new spec. These are guidance: say why when you exceed them. A tiny change (about 100 lines or fewer, 3 criteria or fewer) may leave out the Slices table, and the Decisions table only when no open item touches authority, an interface or something irreversible; it says so.
   - **Unknowns first:** a premise nobody has observed gets slice S0, a spike whose kill criterion is written into the spec before it runs. No other slice starts until its result is in the spec.
   - **Decisions:** propose whether each is reversible; the operator decides. Only one marked reversible may take its recommendation by default (record the default in the tracker). One that changes a criterion, an interface, a data shape or authority blocks the slices it names until answered.
   - **What is not verified:** list each claim nobody has observed.
   - **Review before approval** (`GOVERNANCE.md` §1.3): ask for a Council or a witnessed Panel with Strategist and Red-Team seats, cite it as `reviewed_by: [council:<id>]` or `[panel:<id>]`, and commit its record under `missions/<mission>/councils/` with the spec; `delphi lint` resolves each id. Ids restart every session (`t_0001`): if a committed record already has the same `council_id`, stop and ask instead of citing. A spec with an Authority section also gets `spec-premortem`. Never cite a review that did not happen.
8. Start from `templates/spec-skeleton.md`, with provenance frontmatter and `status: draft`. An agent never marks a spec `approved`.
9. Run `delphi check spec <file>`, then `delphi lint`: a spec numbered 125 or above with no "Options considered" heading gets a warning now and will fail later (spec 124 section 7).

No shell? Call the `check` tool with `kind: spec` and the draft's path; it returns the same verdict.

## Done when
The check passes, every criterion is testable, the simpler alternative is a real alternative, and "Options considered" holds three options that differ in mechanism, each with a test.

## Pitfalls
- Designing the abstraction before the second use case exists.
- Acceptance criteria that restate the design.
- Silent scope growth: put new ideas in a follow-up list.
- Folding the simpler alternative into "Options considered". They are two sections with two rules.
