---
name: roadmap-sequencing
description: "Order multi-phase work into milestones along the critical path, with dependencies and weights checked. Use when planning more than one deliverable."
authorized_agents:
  - strategist
input_schema:
  type: object
  required: ["goals"]
  properties:
    goals:
      type: array
      items: {type: string}
      description: "Outcomes to deliver"
    constraints:
      type: string
      description: "Deadlines, capacity or ordering limits"
output_schema:
  type: object
  required: ["roadmap"]
  properties:
    roadmap:
      type: string
      description: "Path of the validated milestone plan"
    critical_path:
      type: array
      items: {type: string}
      description: "Milestone ids in order"
required_permissions:
  - read:all
  - write:missions
---
# Roadmap Sequencing

Sequence by dependency and risk, not by enthusiasm.

## When to use
- Several deliverables depend on each other, or a goal spans multiple missions.

## Steps
1. Turn each goal into milestones that ship something usable. A milestone with no observable result is a task.
2. Record real dependencies only: what must exist before this can start.
3. Give each milestone a `weight` for relative effort and, where it applies, link the spec it delivers.
4. Put the riskiest unknown early, where discovering it is cheap, and anything the operator must decide or provide ahead of the work that needs it.
5. Write the plan from `templates/milestones.yaml` and run `delphi check dag <file> --repo .`. It rejects cycles and unknown ids, then prints the parallel waves and the critical path.
6. Read the critical path. If it is longer than the constraints allow, cut scope or break a dependency; do not just reorder.

No shell? Call the `check` tool with `kind: dag` and the plan's path; it returns the same verdict.

## Done when
The check passes, the critical path is named, and each milestone states what is true when it ships.

## Pitfalls
- A flat list presented as a sequence.
- Everything on the critical path: that is a sign dependencies are overstated.
