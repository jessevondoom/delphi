---
name: tdd-execution
description: "Implement against a spec's acceptance criteria with red-green-refactor, proving each new test fails first. Use for any behaviour change."
authorized_agents:
  - builder
input_schema:
  type: object
  required: ["spec"]
  properties:
    spec:
      type: string
      description: "Approved spec path"
    criterion:
      type: string
      description: "The acceptance criterion being implemented"
output_schema:
  type: object
  required: ["result"]
  properties:
    result:
      type: string
      description: "Tests added, red and green evidence, and files changed"
required_permissions:
  - read:all
  - write:worktree
  - shell:scoped_scripts
---
# TDD Execution

Tests written after the code prove only that the code does what it does. Write the failing test first.

## When to use
- Any change in behaviour, a bug fix, or a new feature with acceptance criteria.
- Not for pure formatting or a comment.

## Steps
1. Pick one acceptance criterion. Write the smallest test that expresses it.
2. **Red.** Run the test and read the failure. It must fail for the reason you expect; a test that fails to compile or errors on setup is not red yet.
3. **Green.** Write the simplest change that passes. Do not add behaviour no test asks for.
4. **Refactor.** With the suite green, remove duplication and improve names. Run the suite again.
5. Repeat for the next criterion. For a bug, the first test reproduces the bug.
6. Before finishing, run the project's full gates (tests, lints, formatters) and read the output unfiltered. Never chain a commit after a command whose failure a pipe could hide.

## Done when
Every criterion has a test that was seen failing and is now passing, and the full suite is green.

## Pitfalls
- Editing a test to make it pass instead of fixing the code.
- Rerunning a failing test until it passes: investigate flakiness, and quote the evidence if you conclude it is one.
- Spec and code disagree: stop and report drift rather than working around it.
