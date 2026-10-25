---
name: documentation-architecture
description: "Design and maintain README, docs and guides so they match the code and specs, with working links and examples. Use when documentation is added or drifts."
authorized_agents:
  - writer
input_schema:
  type: object
  required: ["audience"]
  properties:
    audience:
      type: string
      description: "Who the document is for"
    sources:
      type: array
      items: {type: string}
      description: "Specs and code it must agree with"
output_schema:
  type: object
  required: ["docs"]
  properties:
    docs:
      type: string
      description: "Paths of the documents created or changed"
required_permissions:
  - read:all
  - write:worktree
---
# Documentation Architecture

Documentation is a map. It must match the territory and be findable.

## When to use
- Writing or restructuring a README, guide or reference, or after a behaviour change that docs describe.

## Steps
1. Decide who reads it and what they are trying to do. Organize by task, not by internal structure.
2. Put the shortest path to success first: what it is, how to install, one working example.
3. Read the code and the specs before writing. Document only what exists and works; mark planned features as planned.
4. Test every command and example by running it, or label it as unverified. Copy output from real runs.
5. Link to the source of truth (spec sections, files) instead of restating it, so there is one place to update.
6. Run `delphi check kb .` to catch broken links and stale references, then fix them.

## Done when
A new reader can complete the main task from the page alone, and every link and example resolves.

## Pitfalls
- Describing the intended design as if built.
- Duplicating a spec in prose, which then drifts.
