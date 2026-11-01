---
name: competitive-swot-analysis
description: "Execute systematic SWOT, Porter's Five Forces, and Wardley mapping analyses on technical and market positions. Use when evaluating competitive dynamics, platform bets, or product viability."
authorized_agents:
  - strategist
  - investigator
input_schema:
  type: object
  required: ["target"]
  properties:
    target:
      type: string
      description: "Subject entity, technology, product, or architectural initiative"
    competitors:
      type: array
      items: {type: string}
      description: "Direct or indirect alternatives, substitutes, or market incumbents"
output_schema:
  type: object
  required: ["competitive_brief"]
  properties:
    competitive_brief:
      type: string
      description: "Synthesized SWOT matrix, Five Forces rating, Wardley mapping, and strategic positioning"
required_permissions:
  - read:all
  - write:missions
---
# Competitive SWOT Analysis

Evaluate technical architectures, product initiatives, and market positions using rigorous SWOT, Porter's Five Forces, and Wardley mapping.

## When to use
- Before launching major product or architectural initiatives facing existing market solutions or internal alternatives.
- When assessing vendor dependencies, platform lock-in risks, or commoditization trajectories.
- During quarterly or milestone strategic reviews to evaluate moat durability.

## Steps
1. **Frame Value Chain & Scope:** Define the exact user need, system boundary, and value chain components.
2. **Perform Wardley Evolution Mapping:** Map components along visibility (user-facing to infrastructure) and evolution (genesis, custom-built, product/rental, commodity/utility).
3. **Assess Porter's Five Forces:** Rate competitive rivalry, supplier power, buyer power, threat of substitution, and barriers to new entry.
4. **Construct Evidence-Backed SWOT:**
   - *Strengths:* Proven internal capabilities, performance advantages, architectural efficiencies.
   - *Weaknesses:* Technical debt, resource bottlenecks, missing capabilities, latency/cost overheads.
   - *Opportunities:* Market gaps, regulatory shifts, open-source tailwinds, architectural paradigms.
   - *Threats:* Competitor moves, API deprecations, platform commoditization, talent attrition.
5. **Cross-Examine Matrix Cells:** Convert SWOT findings into tactical pairings: SO (attack), ST (defend), WO (improve), WT (mitigate).
6. **Formulate Strategic Recommendations:** Deliver defensible strategic guidance backed by empirical data and verification sources.

## Guidelines
- **Zero unverified claims:** Every strength or weakness must cite benchmarks, API capabilities, or primary market documentation.
- **Beware confirmation bias:** Give equal diligence to identifying fatal weaknesses and competitor strengths.
- **Dynamic evolution focus:** Recognize which components are rapidly commoditizing and avoid building custom solutions for utilities.

## Output format
A competitive intelligence dossier containing:
- **Executive Overview:** One-paragraph summary of strategic standing.
- **Wardley Value Chain Map:** Value chain components categorized by evolution tier.
- **Porter's Five Forces Summary:** Ranked forces with supporting evidence.
- **SWOT Matrix:** Categorized grid with empirical citations.
- **Strategic Directives:** Prioritized actions based on matrix cross-examination.
