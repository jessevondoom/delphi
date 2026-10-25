---
name: dynamic-model-scaling
description: "Choose the model tier and thinking budget for a handoff from task signals and retry history. Use when writing a routing directive or after a failed attempt."
authorized_agents:
  - orchestrator
input_schema:
  type: object
  required: ["task"]
  properties:
    task:
      type: string
      description: "What is being dispatched"
    signals:
      type: string
      description: "Context size, ambiguity, prior attempts and failures"
output_schema:
  type: object
  required: ["model_tier"]
  properties:
    model_tier:
      type: string
      description: "The tier for the handoff"
    reason:
      type: string
      description: "One line naming the signal that decided it"
required_permissions:
  - read:all
---
# Dynamic Model Scaling

Spend capability where the task needs it and nowhere else. Routing is by tier; the runtime resolves tiers to models.

## When to use
- Writing the `routing_directive` of a handoff.
- A task failed verification, or hit its turn budget, and needs a retry decision.

## Steps
1. Read `context/model-capabilities.md` for what each class is good at. The classes are `fast`, `workhorse` and `frontier`; config decides which concrete model serves each.
2. Start from the cheapest class that fits: mechanical lookups and triage are `fast`, ordinary implementation is `workhorse`.
3. Raise the class or the thinking budget for: genuine ambiguity, a large cross-file context, a design decision, or a concurrency or security question. Work that touches GOVERNANCE.md, security boundaries, state machines or update migrations goes to `frontier` with a high budget.
4. After a failed verification gate on a `fast` or `workhorse` model, never retry with the same model and budget. Raise one of them and say which.
5. For review and critique, choose a different model family from the author's when more than one is configured. With one family, use a different class or a fresh context and mark the review `homogeneous`.
6. Put the class in `routing_directive.model_tier` and one line of reason in `scaling_rationale`. Never write model IDs or endpoints; the linter rejects both.

## Done when
The directive names a class, the reason cites an observed signal, and a retry changed something (class, budget, context or instructions) rather than repeating the same call.

## Pitfalls
- Escalating on the first failure without reading why it failed.
- Giving every task the top tier "to be safe".
