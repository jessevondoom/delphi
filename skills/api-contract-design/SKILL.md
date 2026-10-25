---
name: api-contract-design
description: "Design a provider-neutral interface, IPC message or CLI grammar with a JSON Schema, examples and error cases. Use when components must agree on a shape."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["interface"]
  properties:
    interface:
      type: string
      description: "The boundary being designed"
    consumers:
      type: array
      items: {type: string}
      description: "Who calls it"
output_schema:
  type: object
  required: ["contract"]
  properties:
    contract:
      type: string
      description: "Schema, examples and error cases, with where they were written"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# API Contract Design

A contract is what two pieces of code agree on. Make it small, explicit, and hard to misuse.

## When to use
- A new message, command, file format, or function boundary that more than one component depends on.

## Steps
1. List the callers and what each needs. Design for those, not for imagined future ones.
2. Define the shape as a JSON Schema (draft 2020-12) with required fields, types, and bounds. Prefer closed enums to free strings.
3. Specify every error: what the caller sees, whether it is retryable, and what state is left behind.
4. Make it versionable: say how an old reader handles a new field and how a breaking change is signalled.
5. Add one valid example, and at least two invalid ones with the reason each is rejected.
6. For a CLI, give the grammar, defaults, exit codes, and what is written to stdout versus stderr.
7. Keep provider names and model IDs out of the contract; use tiers and capabilities.

## Done when
A second person could implement the other side from the contract alone, and the invalid examples are rejected by the schema.

## Pitfalls
- Optional fields that interact in undocumented ways.
- Errors described as "returns an error".
