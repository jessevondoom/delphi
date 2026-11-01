---
name: oblique-strategies
description: "Apply lateral thinking prompts, Oblique Strategies, and TRIZ contradiction matrices to break architectural deadlocks. Use when standard designs hit intractable trade-offs or creative blocks."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["problem"]
  properties:
    problem:
      type: string
      description: "The architectural deadlock, contradiction, or trade-off to resolve"
    constraints:
      type: array
      items: {type: string}
      description: "Implicit or explicit constraints currently assumed"
output_schema:
  type: object
  required: ["lateral_synthesis"]
  properties:
    lateral_synthesis:
      type: string
      description: "Synthesized alternative paradigms, inverted assumptions, and concrete resolution paths"
required_permissions:
  - read:all
  - write:missions
---
# Oblique Strategies

Break conceptual deadlocks by challenging implicit constraints, inverting core assumptions, and applying TRIZ contradiction principles.

## When to use
- Standard architectural approaches produce conflicting requirements (e.g., latency vs consistency, simplicity vs flexibility) with no clear winner.
- Council debates reach a stalemate between roles over mutually exclusive constraints.
- The design feels incrementally trapped in the assumptions of an existing implementation.

## Steps
1. **Isolate the Core Contradiction:** Express the deadlock as a TRIZ-style technical contradiction (improving parameter X worsens parameter Y) or physical contradiction (the system must be simultaneously stateful and stateless).
2. **Map Implicit Constraints:** Enumerate all unstated assumptions (e.g., ordering guarantees, synchronization boundaries, persistent disk storage, synchronous RPC calls).
3. **Draw Lateral Prompts:** Apply lateral heuristics and Eno-style oblique prompts (e.g., "Honor thy error as a hidden intention", "Reverse roles", "Discard the most important constraint", "What would the simplest 1970s Unix tool do?").
4. **Invert and Reframe:** For each constraint, deliberately invert it: what if we dropped strict ordering? What if the client resolved conflicts instead of the server? What if data is never mutated, only appended?
5. **Formulate Divergent Prototypes:** Draft two to three distinct architectural sketches that resolve the contradiction by reframing the problem space entirely.
6. **Filter for Viability:** Evaluate which radical alternative survives Delphi verification gates and can be specified cleanly with `Simpler Alternative Considered`.

## Guidelines
- **Ground weirdness in mechanics:** Unconventional ideas must resolve into concrete data flows, invariant boundaries, or state transitions.
- **Attack premises, not symptoms:** If two requirements clash, question whether requirement B should exist at all.
- **Keep it bounded:** Oblique exploration is a tool to break deadlocks, not an excuse for perpetual speculation. Limit to 3 candidate reframings.

## Output format
A structured lateral analysis document containing:
- **Contradiction Statement:** The exact conflict or impasse.
- **Inverted Assumptions:** The unwritten constraints challenged and overturned.
- **Candidate Divergences:** 2-3 concrete architectural alternatives with trade-offs.
- **Recommended Reframing:** The suggested resolution path for spec authoring.
