---
name: adversarial-test-gen
description: "Generate boundary, malformed-input, concurrency and fault-injection tests that try to break a change, and report which ones do. Use after the happy path passes."
authorized_agents:
  - red-team
input_schema:
  type: object
  required: ["target"]
  properties:
    target:
      type: string
      description: "Code, function or interface to attack"
    spec:
      type: string
      description: "Contract it claims to meet"
output_schema:
  type: object
  required: ["tests"]
  properties:
    tests:
      type: array
      items: {type: string}
      description: "Test cases written, with results and any failures found"
required_permissions:
  - read:all
  - shell:scoped_scripts
---
# Adversarial Test Generation

The author tested what they expected. Test what they did not.

## When to use
- A change has its happy-path tests and needs a hostile pass, especially parsers, file handling, permissions and concurrency.

## Steps
1. Read the contract and list its promises. Each promise is a target.
2. For each input, try the edges: empty, maximum, one past the limit, negative, unicode and odd encodings, very long, duplicate, and structurally wrong.
3. For state, try ordering and repetition: call twice, call out of order, cancel midway, run two at once, kill it halfway and resume.
4. For files and paths, try traversal, symlinks, case differences, and names that look like control files.
5. Write each probe as a reproducible test with the exact input and the expected behaviour from the contract. Run them and record the outcome.
6. Report the failures first, each with the smallest reproduction. Hand fixes back; do not edit the production code.

## Done when
Each failing probe has a minimal reproduction, and passing probes are listed so the coverage is visible.

## Pitfalls
- Random inputs with no expectation: say what correct looks like.
- Fixing the code yourself: the author needs to see the failure.
