---
name: safe-refactoring
description: "Restructure code while keeping its public behaviour and API surface unchanged, in small steps with tests green. Use for cleanup, renames and extractions."
authorized_agents:
  - builder
input_schema:
  type: object
  required: ["goal"]
  properties:
    goal:
      type: string
      description: "The structural change wanted"
    scope:
      type: array
      items: {type: string}
      description: "Files or modules in play"
output_schema:
  type: object
  required: ["result"]
  properties:
    result:
      type: string
      description: "Summary of the change and proof the surface is unchanged"
required_permissions:
  - read:all
  - write:worktree
  - shell:scoped_scripts
---
# Safe Refactoring

A refactor changes structure, not behaviour. If behaviour changes, it is a feature and needs a spec and a test.

## When to use
- Renaming, extracting, moving, or simplifying code with existing coverage.

## Steps
1. Make sure the code is covered. If not, add characterization tests that pin current behaviour before moving anything.
2. Record the public surface (exported items, signatures, CLI flags, file formats) so you can compare after.
3. Change in small steps, running the tests after each. One kind of move per commit: rename, move, extract, or inline.
4. Do not fix bugs or add features in the same change. Note them separately.
5. At the end, compare the public surface with the record. Any difference is either reverted or reported as a deliberate break with its callers.
6. Run the full gates.

## Done when
Tests pass unchanged (apart from names the refactor required), and the surface comparison shows no unintended change.

## Pitfalls
- A big-bang rewrite that cannot be reviewed.
- Editing tests to fit the new structure when they should have caught the break.
