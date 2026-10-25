---
name: task-decomposition
description: "Turn an approved spec or goal into a task DAG with a cognitive tier and spec link per task. Use before dispatching any multi-step mission."
authorized_agents:
  - orchestrator
input_schema:
  type: object
  required: ["goal"]
  properties:
    goal:
      type: string
      description: "Path to the approved spec or the operator's goal"
    mission:
      type: string
      description: "Mission id the plan belongs to"
output_schema:
  type: object
  required: ["plan"]
  properties:
    plan:
      type: string
      description: "Path of the validated plan file under the mission"
    waves:
      type: integer
      description: "Number of parallel waves"
required_permissions:
  - read:all
  - write:missions
---
# Task Decomposition

Break a goal into the smallest set of tasks that can each be handed to one agent and verified on its own.

## When to use
- A request needs more than one agent or more than one sitting.
- Not for a single edit or a question: just do it.

## Steps
1. Read the approved spec (or the goal). If there is no approved spec for non-trivial work, stop and route to Architect (No Spec, No Build).
2. List deliverables, not activities: each task ends in an artifact someone can check (a file, a passing test, a report).
3. Give each task a short `id`, one owner-shaped sentence, and a `tier`: `deep-reasoning` (design, ambiguity), `code-craft` (implementation), `high-context-discovery` (broad reading), `fast-triage` (mechanical).
4. Add `depends_on` only for a real data or ordering dependency. Fewer edges means more parallel work.
5. Link each task to the spec it implements (`spec: specs/NNN-name.md`) and give a rough `weight` (1 small, 5 large).
6. Write the plan from `templates/task-dag.yaml` to `missions/<mission>/plan.yaml`.
7. Run `delphi check dag missions/<mission>/plan.yaml --repo .` and fix every failure. A cycle means two tasks are really one, or a dependency points the wrong way.

No shell? Call the `check` tool with `kind: dag` and the plan's path; it returns the same verdict.

## Done when
The check passes, every task has a verifiable deliverable, and the critical path it prints matches your expectation of the longest chain.

## Pitfalls
- Tasks named after phases ("research", "implement") instead of deliverables.
- Serial chains where tasks only share a topic, not data.
- Choosing a tier by habit: the tier sets cost, so justify `deep-reasoning`.
