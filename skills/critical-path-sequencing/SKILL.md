---
name: critical-path-sequencing
description: "Sequence task dependency DAGs, calculate float and critical path bottlenecks, and balance Little's Law WIP limits. Use when optimizing multi-agent execution order or resolving project stalls."
authorized_agents:
  - administrator
  - strategist
input_schema:
  type: object
  required: ["tasks"]
  properties:
    tasks:
      type: array
      items:
        type: object
        required: ["id", "depends_on"]
        properties:
          id: {type: string}
          depends_on: {type: array, items: {type: string}}
          estimate: {type: string}
output_schema:
  type: object
  required: ["critical_path", "execution_waves"]
  properties:
    critical_path:
      type: array
      items: {type: string}
      description: "The zero-float sequence of tasks dictating minimum project completion time"
    execution_waves:
      type: array
      items: {type: array, items: {type: string}}
      description: "Parallel task waves safe to dispatch concurrently without violating WIP limits"
required_permissions:
  - read:all
  - write:missions
---
# Critical Path Sequencing

Calculate critical paths, early/late starts, and dependency waves across task DAGs.

## When to use
- When scheduling multi-agent dispatches across dependent tasks.
- When identifying why a release is blocked or delayed.
- When organizing parallel exploratory work by `scout` or `investigator` alongside `builder`.

## Steps
1. **DAG Validation:** Check that task dependencies form a Directed Acyclic Graph with zero circular references.
2. **Forward Pass (Early Start & Finish):** Compute earliest possible start and completion times for every task.
3. **Backward Pass (Late Start & Finish):** Compute latest allowable start and finish times without delaying overall completion.
4. **Float & Critical Path Calculation:** Tasks where `Late Finish - Early Finish = 0` form the Critical Path.
5. **WIP-Bounded Wave Formulation:** Group independent tasks into concurrent waves bounded by Little's Law.

## Output format
- **Critical Path:** Ordered list of critical tasks.
- **Concurrent Waves:** Parallel execution stages.
- **Float Ledger:** Available slack for non-critical tasks.
