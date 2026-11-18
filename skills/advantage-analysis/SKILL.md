---
name: advantage-analysis
description: "Test each source of durable advantage for a benefit and a barrier, with evidence; map how components evolve; pick what to eliminate, reduce, raise or create. Use to judge a moat or a market bet."
authorized_agents:
  - strategist
input_schema:
  type: object
  required: ["subject"]
  properties:
    subject:
      type: string
      description: "The product, platform bet or initiative whose advantage is in question"
    rivals:
      type: array
      items: {type: string}
      description: "The alternatives, substitutes or incumbents it is measured against"
    evidence:
      type: array
      items: {type: string}
      description: "Paths or sources the claims may rest on"
output_schema:
  type: object
  required: ["analysis"]
  properties:
    analysis:
      type: string
      description: "The advantage table, the component map and the four-actions grid, as text or as the path they were written to"
    unsupported:
      type: array
      items: {type: string}
      description: "Advantages that were claimed and struck for lack of evidence"
required_permissions:
  - read:all
  - write:missions
---
# Advantage Analysis

An advantage is durable only when it gives a benefit **and** something stops a rival from copying it. This skill tests each advantage someone names for both halves and for evidence, then looks at where the parts of the product are heading and what to change. It replaces a strengths-and-weaknesses grid, which lists and does not test.

## When to use
- Someone says "our moat is X" or asks whether a position can be defended.
- A platform bet, a build-or-buy choice or a dependency on one vendor is being judged.
- A strategy needs to know which parts of the product are worth custom work and which are becoming a commodity.

## Steps
1. **State the subject and the rivals.** Include what buyers do today without any product. Work from what the workspace holds first. A claim about a rival, a market or a number carries its source, or the label `from model memory, not sourced`; ask the scout when it matters.
2. **List the advantages that are claimed**, in the words of whoever claimed them. Do not add your own yet.
3. **Test each one. All three are required, or it is struck:**
   - **Benefit:** what it changes for the business that could be measured: a lower cost, a higher price that holds, or less to spend to win a buyer.
   - **Barrier:** why a capable rival that sees it cannot or will not do the same within a few years. "They have not yet" is not a barrier.
   - **Evidence:** a source for the benefit and a source for the barrier. A struck claim goes in the `unsupported` list with what evidence would restore it.
4. **Name the kind of barrier** for each that survives. Seven kinds are commonly used: cost that falls with scale; value that grows with each user; a model an incumbent cannot copy without harming its own business; the cost a buyer pays to switch; a brand buyers pay more for; a resource rivals cannot get; and a way of working that takes years to build. If it fits none, say what it is in plain words. Do not force a fit.
5. **Map the components.** List what the product needs in order to serve the buyer, from what the buyer sees down to what it runs on. Place each at one of four stages: new and poorly understood, custom-built, sold as a product, or a commodity anyone can rent. Say which way each is moving and what shows that. Flag any custom work on a component that is already a commodity, and any advantage that rests on a component about to become one.
6. **Fill the four-actions grid** against what the rivals offer: what to **eliminate** that the field takes for granted, what to **reduce** well below the usual level, what to **raise** well above it, and what to **create** that nobody offers. Each entry names the buyer it is for and what it costs or saves. An empty cell is allowed; say that it is empty on purpose.
7. **Report:** the advantages that survived with their barrier and evidence, the ones struck, the component flags, the grid, and the one judgement you are least sure of.

## Layout
```markdown
# Advantage: <subject>

## Claimed advantages
| Advantage | Benefit | Barrier | Kind | Evidence | Verdict |
| :-- | :-- | :-- | :-- | :-- | :-- |
| <as claimed> | <measurable> | <why not copied> | <kind or plain words> | <source for each half> | stands / struck |

## Components
| Component | Stage | Moving toward | Sign | Flag |
| :-- | :-- | :-- | :-- | :-- |

## Four actions
| Eliminate | Reduce | Raise | Create |
| :-- | :-- | :-- | :-- |
```

## Done when
Every claimed advantage has a verdict; every one that stands has a benefit, a barrier and a source for each; every struck one is in `unsupported` with the evidence that would restore it; every component has a stage and a direction with a sign; the grid's four cells are filled or marked empty on purpose; and no claim about the outside is left without a source or the label `from model memory, not sourced`.

## Pitfalls
- A benefit with no barrier. Being better today is not an advantage that lasts.
- Evidence for the benefit offered as evidence for the barrier.
- Finding all seven kinds. Most products have one or none; say so.
- Rating components by how proud the team is of them instead of by what the market offers.
- A grid where every cell says "raise". Raising costs something; say what is eliminated or reduced to pay for it.
- Writing the analysis from memory of a market. Without sources it is a guess, and the label must say so.

## Sources
The benefit-and-barrier test and the seven kinds of barrier are commonly attributed to Hamilton Helmer, the stages of component evolution to Simon Wardley, and the eliminate, reduce, raise, create grid to W. Chan Kim and Renée Mauborgne. This text was written from secondary summaries, not from the primary works, and has not been checked against them. It cites no primary work until the operator ratifies the sources (`specs/124` §12, OD-3).
