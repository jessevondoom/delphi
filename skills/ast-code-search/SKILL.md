---
name: ast-code-search
description: "Find where a symbol is defined, used and how data flows to it, using structural search before text search. Use when locating code to understand or change."
authorized_agents:
  - architect
  - investigator
input_schema:
  type: object
  required: ["symbol"]
  properties:
    symbol:
      type: string
      description: "Function, type or pattern to find"
    scope:
      type: string
      description: "Directory to search"
output_schema:
  type: object
  required: ["findings"]
  properties:
    findings:
      type: array
      items: {type: string}
      description: "Locations as path:line with a one-line role for each"
required_permissions:
  - read:all
  - shell:scoped_scripts
---
# AST and Code Search

Find the definition first, then the callers, then follow the data. Report locations, not file dumps.

## When to use
- You need to know where something is defined, who calls it, or what would break if it changed.

## Steps
1. Locate the definition. Prefer a structural tool (`ast-grep`, tree-sitter, the language server) over plain text when the name is common; fall back to `rg` with word boundaries.
2. Find the callers and implementors. Separate production code, tests and docs.
3. Follow the data: where the value is created, transformed, stored, and read. Note any boundary it crosses (process, file, network).
4. Check for dynamic use that static search misses: string-built names, reflection, macros, configuration.
5. Report each finding as `path:line` with one line saying what it is. Group by role (definition, caller, test).
6. Say what you could not determine, and why.

## Done when
Someone can open each location and see the claimed role, and the list says what a search could not cover.

## Pitfalls
- Reporting every text match as a call site.
- Quoting large excerpts instead of pointing.
