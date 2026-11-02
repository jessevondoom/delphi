---
name: work-breakdown-structures
description: "Decompose complex, ambiguous epics into Mutually Exclusive, Collectively Exhaustive (MECE) task hierarchies. Use when scoping initiatives or planning sprint structures."
authorized_agents:
  - administrator
  - orchestrator
input_schema:
  type: object
  required: ["epic_scope"]
  properties:
    epic_scope:
      type: string
      description: "Description and boundary conditions of the epic or system to decompose"
    methodology:
      type: string
      description: "Delivery methodology: kanban, scrum, or shape-up"
output_schema:
  type: object
  required: ["wbs_tree"]
  properties:
    wbs_tree:
      type: string
      description: "Structured hierarchical work breakdown structure with concrete acceptance criteria per leaf task"
required_permissions:
  - read:all
  - write:missions
---
# Work Breakdown Structures

Decompose ambiguous initiatives into structured, MECE (Mutually Exclusive, Collectively Exhaustive) task hierarchies.

## When to use
- At the start of a mission or epic to establish clear task boundaries.
- When scope is too broad for a single specialist agent or PR.
- When identifying parallel execution streams versus serial dependencies.

## Steps
1. **Define Bounded Outcomes:** State the single primary deliverable and explicit non-goals (what will NOT be built).
2. **First-Level Decomposition (Phase/Subsystem):** Divide the initiative into 3–5 distinct architectural or operational domains.
3. **Leaf Task Isolation:** Break subsystems down into independently testable tasks, each targeting a single pull request.
4. **MECE Verification:** Verify that leaf tasks have zero overlap (Mutually Exclusive) and together cover 100% of the required scope (Collectively Exhaustive).
5. **Attach Definitions of Done:** Every leaf task must specify binary pass/fail verification gates.

## Output format
- **Epic Objective:** The target capability.
- **Decomposition Tree:** Numbered hierarchy (1.0, 1.1, 1.1.1).
- **Verification Gates:** Specific test commands required per leaf item.
