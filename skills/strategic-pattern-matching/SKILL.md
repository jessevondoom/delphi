---
name: strategic-pattern-matching
description: "Map tactical challenges to historical precedents, market campaigns, and strategic doctrine to uncover asymmetric advantages. Use when sequencing initiatives or positioning against competition."
authorized_agents:
  - strategist
input_schema:
  type: object
  required: ["situation"]
  properties:
    situation:
      type: string
      description: "Current market, product, or organizational challenge requiring strategic direction"
    objectives:
      type: array
      items: {type: string}
      description: "Desired strategic outcomes, resource constraints, and competitive context"
output_schema:
  type: object
  required: ["strategic_doctrine"]
  properties:
    strategic_doctrine:
      type: string
      description: "Historical analogies, asymmetric maneuvers, sequenced milestones, and risk boundaries"
required_permissions:
  - read:all
  - write:missions
---
# Strategic Pattern Matching

Apply strategic precedents from military history, classical doctrine, and technology turnarounds to construct asymmetric advantages.

## When to use
- Sequencing complex multi-phase product initiatives against entrenched competitors or legacy standards.
- Designing adoption wedges, Trojan Horse distribution mechanisms, or platform ecosystem transitions.
- Evaluating organizational or technological trade-offs where direct head-to-head competition is unfavorable.

## Steps
1. **Define Strategic Landscape:** Clarify relative positioning, resources, terrain (market/ecosystem dynamics), and adversary strengths.
2. **Identify Historical Analogs:** Match the situation against classical archetypes (e.g., Fabian delay, oblique order, Blitzkrieg encirclement, Moore's crossing the chasm, disruptive low-end footholds).
3. **Extract Invariant Principles:** Separate the specific historical context from underlying strategic dynamics (e.g., concentration of force, interior lines of communication, switching cost asymmetries).
4. **Formulate Asymmetric Maneuvers:** Design maneuvers that leverage competitor inertia, bypass frontal defenses, and exploit unmonitored flanks (e.g., developer ergonomics, open-source Trojan horses).
5. **Establish Phased Milestones:** Sequence actions into decisive stages with clear exit criteria and pivot triggers.
6. **Assess Vulnerabilities:** Stress-test the strategy against competitor retaliation, supply constraints, and over-extension.

## Guidelines
- **Avoid superficial analogies:** Ensure the structural dynamics (incentives, costs, feedback loops) genuinely correspond.
- **Asymmetry over brute force:** If the strategy requires matching a larger incumbent dollar-for-dollar or feature-for-feature, reject it.
- **Actionable sequencing:** Strategy is meaningless without an ordered dependency path and clear verification gates.

## Output format
A strategic directive document comprising:
- **Situation & Landscape Assessment:** The structural field of play and power asymmetries.
- **Historical & Industry Analogues:** 2-3 matched precedents and extracted doctrinal principles.
- **Recommended Asymmetric Maneuver:** Concrete plan of action, wedge strategy, and positioning.
- **Execution Cadence & Tripwires:** Critical-path milestones and abort/pivot metrics.
