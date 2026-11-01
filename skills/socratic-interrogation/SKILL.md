---
name: socratic-interrogation
description: "Subject proposals, specs, and council arguments to disciplined Socratic interrogation with minimalist, probing questions. Use when evaluating critical design premises or high-contention claims."
authorized_agents:
  - red-team
input_schema:
  type: object
  required: ["target"]
  properties:
    target:
      type: string
      description: "Proposal, spec, or council argument to interrogate"
    contention_points:
      type: array
      items: {type: string}
      description: "Key areas of uncertainty, dispute, or implicit assumptions"
output_schema:
  type: object
  required: ["interrogation"]
  properties:
    interrogation:
      type: string
      description: "Curated sequence of probing questions, targeted vulnerabilities, and evidentiary demands"
required_permissions:
  - read:all
  - write:missions
---
# Socratic Interrogation

Probe designs, council arguments, and assumptions using disciplined, minimalist Socratic inquiries to unmask hidden flaws.

## When to use
- During Council Round 2 cross-examinations when debating architectural or strategic alternatives.
- When reviewing specs that make bold claims of performance, safety, or simplicity without supporting proof.
- When an author or agent team exhibits confirmation bias or consensus complacency.

## Steps
1. **Extract Core Thesis & Pillars:** Strip away rhetoric to expose the 1-3 foundational assertions the proposal stands on.
2. **Formulate Precision Probes:** Construct terse, penetrating questions targeting the weakest joints (e.g., "What happens when this lock is held during crash?", "Why does this requirement exist?", "Defend why this survives 10x traffic.").
3. **Demand Operational Definitions:** Force ambiguous terms ("scalable", "resilient", "eventually consistent") into observable, falsifiable metrics.
4. **Map Second-Order Consequences:** Trace what must become true if the thesis is accepted: what dependencies, operational burdens, and failure states are inherited?
5. **Identify Falsification Criteria:** Ask what concrete observation or test would prove the design wrong.
6. **Compile Interrogation Dossier:** Deliver a structured inquiry requiring explicit evidence rather than speculative defense.

## Guidelines
- **Minimalist and unyielding:** Avoid verbose rebuttals. Short questions force authors to reveal the depth of their reasoning.
- **Inquire rather than pontificate:** Instead of declaring "this will fail under high load", ask "What bounds queue depth when downstream latency spikes to 2 seconds?".
- **Focus on trust boundaries and contention:** Zero in on shared state, concurrency, security boundaries, and rollback strategies.

## Output format
An interrogation dossier formatted as:
- **Target Assertions:** Explicit claims under examination.
- **Socratic Probes:** 3-7 numbered, sharp inquiries targeting assumptions, invariants, and failure modes.
- **Falsification Demands:** Specific scenarios and benchmarks required to validate the claim.
