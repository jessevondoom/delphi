---
name: repo-cartography
description: "Map an unfamiliar repository: entry points, module boundaries, build and test commands, in a pointer map under 300 lines. Use when first meeting a codebase."
authorized_agents:
  - architect
  - scout
input_schema:
  type: object
  required: ["root"]
  properties:
    root:
      type: string
      description: "Repository or directory to map"
    focus:
      type: string
      description: "Area of interest, if any"
output_schema:
  type: object
  required: ["map"]
  properties:
    map:
      type: string
      description: "Path of the pointer map"
required_permissions:
  - read:all
  - write:missions
---
# Repository Cartography

Produce a map someone can navigate by, not a copy of the repository.

## When to use
- First contact with a repository, or before a change that crosses modules.

## Steps
1. Read the manifests and top-level docs: package files, workspace definitions, build scripts, README, CI configuration.
2. Identify entry points (binaries, servers, exported libraries), then the module boundaries and who depends on whom.
3. Find how it is built, tested and run. Verify each command by reading the CI configuration; run it if you may.
4. Note conventions that will trip a newcomer: generated code, vendored directories, feature flags, unusual layouts.
5. Write the map as pointers: `path#Lx-Ly` plus one line each. Keep it under 300 lines and group by concern.
6. List unknowns and the files you did not open.

## Done when
A newcomer can find where a feature lives and how to run its tests from the map alone.

## Pitfalls
- Pasting file contents instead of pointing at them: long dumps lose the reader.
- Describing the intended architecture rather than the actual imports.
