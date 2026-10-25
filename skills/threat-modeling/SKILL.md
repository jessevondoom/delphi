---
name: threat-modeling
description: "Model threats to a design or change with STRIDE plus agent-specific threats such as prompt injection and memory poisoning. Use when a feature crosses a trust boundary."
authorized_agents:
  - red-team
input_schema:
  type: object
  required: ["target"]
  properties:
    target:
      type: string
      description: "Design, spec or diff to model"
    boundaries:
      type: string
      description: "Trust boundaries involved, if known"
output_schema:
  type: object
  required: ["model"]
  properties:
    model:
      type: string
      description: "Path of the threat model with ranked threats and mitigations"
required_permissions:
  - read:all
  - write:missions
---
# Threat Modeling

Find how it breaks under attack while it is still cheap to change.

## When to use
- A feature handles untrusted input, runs commands, calls the network, or moves data between trust levels.

## Steps
1. Draw the data flow in words: sources, processes, stores, and where each crosses a trust boundary. Mark every place external or tainted content enters.
2. For each boundary crossing, apply STRIDE: spoofing, tampering, repudiation, information disclosure, denial of service, elevation of privilege.
3. Add the agent-specific questions: can injected text reach an instruction channel? Can a tainted span trigger a mutating or outbound tool? Can anything write memory or rules that are loaded later? Can a tool be widened beyond the role's scope?
4. For each credible threat, write the attack in one or two sentences, the impact, and the existing control, if any.
5. Rank by impact and likelihood. Propose the cheapest mitigation per top threat, and say which is already covered by a spec or a check.
6. List assumptions and what is out of scope.

## Done when
Every boundary crossing has been considered, and the top threats each have a concrete attack and a mitigation or an accepted risk.

## Pitfalls
- A generic list of threats not tied to this design.
- Stopping at the first finding.
