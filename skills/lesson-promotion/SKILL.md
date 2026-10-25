---
name: lesson-promotion
description: "Turn a verified lesson into a role's lessons.md within the Rule of 25, merging or demoting to make room. Use after a retrospective."
authorized_agents:
  - orchestrator
input_schema:
  type: object
  required: ["lesson"]
  properties:
    lesson:
      type: string
      description: "The candidate lesson and its evidence"
    role:
      type: string
      description: "Role whose lessons it belongs to"
output_schema:
  type: object
  required: ["proposal"]
  properties:
    proposal:
      type: string
      description: "Path of the proposed lessons.md edit"
required_permissions:
  - read:all
  - write:missions
---
# Lesson Promotion

A lesson earns a place by changing behaviour, and the file has room for 25.

## When to use
- A retrospective produced a candidate lesson, or a failure repeated.

## Steps
1. Check provenance. A lesson from external or unverified material is not promoted without deterministic verification or operator ratification.
2. Check for a near-duplicate in the role's `lessons.md` and `context/heuristics.md`. If one exists, merge or sharpen it rather than adding.
3. Write it in the three-line form from `templates/lesson-rule.md`: **T** (when it applies), **D** (what to do), **F** (the failure it prevents). Keep the body under 350 characters.
4. If the file is full, merge, demote, or promote the weakest rule to a script. Update `active_rules` in the frontmatter.
5. Run `delphi check rules <file>`; it enforces the 25-rule cap, the size cap, the T/D/F lines, and the `active_rules` count.
6. Submit as a proposal; lessons change only through ratification.

## Done when
The check passes and the proposal explains which evidence justified the lesson and which rule made room.

## Pitfalls
- Vague advice ("be careful with tests"): a rule must say when and what.
- Letting the count drift from `active_rules`.
